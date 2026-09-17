//! 聚合服务：定时 tick，把采集器/事件/额度融合为岛快照（IslandSnapshot）。
//! 调度策略：每 tick（建议 10s）做采集+状态计算；GLM 额度每 5 分钟刷新一次，
//! 刷新失败降级显示最近快照（红线④）。T8/T9 由 Tauri 后台线程驱动并向前端广播。
//!
//! 2026-09-17 审查优化：
//!   ① 适配器注册表（审查 3.6）：Vec<Box<dyn AgentAdapter>>，新 Agent 即插即用；
//!   ② degraded 标志（审查 1.1）：连续多轮采集源失败 → 快照带降级位，前端可见，
//!      配合文件日志终结"静默失明"；
//!   ③ 进程枚举复用 System 实例且降频到 30s（审查 2.2.4）；
//!   ④ 会话 token/模型改批量查询（审查 2.2.2，消除逐会话 N+1）；
//!   ⑤ hook 事件文件消费失败留痕 + 已消费完超限自动轮转（审查 1.2）。

use std::collections::HashMap;
use std::sync::Arc;

use crate::collector::claude_code::ClaudeCodeAdapter;
use crate::collector::hook_events;
use crate::collector::zcode::ZcodeAdapter;
use crate::collector::AgentAdapter;
use crate::provider::glm::GlmProvider;
use crate::provider::ProviderAdapter;
use crate::state::{
    aggregate, compute_state, ERROR_FRESH_MS, IslandState, SessionSignals, SessionState,
};
use crate::store::Store;

/// 额度刷新间隔（毫秒）
const QUOTA_REFRESH_MS: i64 = 5 * 60 * 1000;
/// 水位安全余量（毫秒，R4）：并发会话慢刷盘的行 ts 可能略小于其他会话推进的
/// 全局水位，按原始水位过滤会永久丢行；回退 60s 重采，幂等键保证不重复入库
const WATERMARK_MARGIN_MS: i64 = 60 * 1000;
/// 进程枚举间隔（毫秒，审查 2.2.4）：全量刷新开销可观，且进程存亡只影响
/// idle/offline 粒度，30s 精度足够
const PROBE_INTERVAL_MS: i64 = 30 * 1000;
/// 连续失败多少轮判定 degraded（10s/轮，3 轮 ≈ 30s：瞬时抖动不闪红）
const DEGRADE_AFTER_TICKS: u32 = 3;

/// 展示用会话视图（serde 给前端）
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionView {
    pub id: String,
    pub agent: String,
    pub model: Option<String>,
    pub project_dir: Option<String>,
    pub title: Option<String>,
    pub state: SessionState,
    pub session_tokens: i64,
    pub last_activity_at: Option<i64>,
}

/// 展示用额度视图
#[derive(Debug, Clone, serde::Serialize)]
pub struct QuotaView {
    pub provider: String,
    pub window_kind: String,
    pub used_percent: Option<f64>,
    pub reset_at: Option<i64>,
}

/// 岛快照：一次 tick 的完整产出
#[derive(Debug, Clone, serde::Serialize)]
pub struct IslandSnapshot {
    pub sessions: Vec<SessionView>,
    pub island: IslandState,
    pub quotas: Vec<QuotaView>,
    /// GLM 5h 额度已耗尽（100%）：不改写会话状态，由前端驱动胶囊变红/标签红光
    pub quota_exhausted: bool,
    /// 采集源连续失败（审查 1.1）：前端在收缩态提示"采集异常"，
    /// 与"没有会话"区分开——降级必须可见，不允许静默失明
    pub degraded: bool,
    pub generated_at: i64,
}

/// 聚合器（有状态：水位/事件偏移/额度刷新计时/进程探测缓存）
pub struct Aggregator {
    store: Arc<Store>,
    /// Agent 适配器注册表（审查 3.6）：新增 Agent = 实现 trait 后在此登记，
    /// 采集主循环不出现 per-agent if-else
    adapters: Vec<Box<dyn AgentAdapter>>,
    /// hook 事件文件的消费偏移（持久化于 app_settings）
    hook_offset: u64,
    /// 上次额度刷新成功时间
    last_quota_fetch: i64,
    glm: Option<GlmProvider>,
    /// 进程枚举复用实例（审查 2.2.4）：避免每 tick 重建 sysinfo 全量快照
    sys: sysinfo::System,
    /// （zcode 活着， claude 活着）探测缓存
    probe_cache: (bool, bool),
    last_probe_ms: i64,
    /// 连续采集失败轮数（degraded 判定输入）
    fail_streak: u32,
}

impl Aggregator {
    pub fn new(store: Arc<Store>) -> Self {
        let hook_offset = store
            .get_setting("hook_events_offset")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        // GLM 凭据优先级：应用设置（token 非空才生效）> 环境变量 > claude-menu
        // suppliers.json；设置页"留空则继续沿用"= token 为空时回落自动发现链
        let (glm, source) = match (
            store.get_setting("glm_base"),
            store.get_setting("glm_token"),
        ) {
            (Some(base), Some(token)) if !base.is_empty() && !token.is_empty() => {
                (Some(GlmProvider::new(&base, &token)), "应用设置".to_string())
            }
            _ => match GlmProvider::discover() {
                Some(c) => (Some(GlmProvider::new(&c.base, &c.token)), c.source.to_string()),
                None => (None, String::new()),
            },
        };
        // 记录凭据来源供设置页展示"（当前：xxx）"；无凭据时清除旧记录
        store.set_setting("glm_token_source", &source);
        if glm.is_none() {
            log::info!("GLM 凭据未配置：额度功能区停用（设置页可配置，或走自动发现链）");
        }
        Self {
            store,
            adapters: vec![Box::new(ZcodeAdapter::new()), Box::new(ClaudeCodeAdapter::new())],
            hook_offset,
            last_quota_fetch: 0,
            glm,
            sys: sysinfo::System::new(),
            probe_cache: (false, false),
            last_probe_ms: 0,
            fail_streak: 0,
        }
    }

    /// 执行一轮采集+融合，返回岛快照
    pub fn tick(&mut self) -> IslandSnapshot {
        let now = now_ms();
        let mut had_error = false;

        // ① hooks 事件增量（claude-code；文件不存在=未安装增强档，静默降级）
        let mut last_hooks: HashMap<String, (String, i64, Option<String>)> = HashMap::new();
        if let Some(path) = hook_events::events_file_path() {
            match hook_events::read_events(&path, self.hook_offset) {
                Ok((events, new_off)) => {
                    // 偏移无推进时免写库（R9，每 10s 一次的空写没必要）
                    if new_off != self.hook_offset {
                        self.store.set_setting("hook_events_offset", &new_off.to_string());
                    }
                    self.hook_offset = new_off;
                    for ev in events {
                        // 原始事件审计落库 status_events(02-DESIGN §3，R7)
                        self.store.insert_status_event(
                            "claude-code",
                            Some(ev.session_id.as_str()),
                            &ev.hook,
                            &serde_json::to_string(&ev).unwrap_or_default(),
                            ev.ts,
                        );
                        // 每会话保留最新事件
                        last_hooks
                            .entry(ev.session_id.clone())
                            .and_modify(|e| {
                                if ev.ts >= e.1 {
                                    *e = (ev.hook.clone(), ev.ts, ev.message.clone());
                                }
                            })
                            .or_insert((ev.hook.clone(), ev.ts, ev.message.clone()));
                    }
                    // 轮转：仅当本轮已全部消费且超限；归零后回写偏移（审查 1.2）
                    let rotated = hook_events::rotate_if_large(
                        &path,
                        self.hook_offset,
                        hook_events::MAX_EVENT_FILE_BYTES,
                    );
                    if rotated != self.hook_offset {
                        self.hook_offset = rotated;
                        self.store.set_setting("hook_events_offset", "0");
                    }
                }
                Err(e) => {
                    log::warn!("hook 事件文件读取失败（本轮按无事件处理）：{e:#}");
                    had_error = true;
                }
            }
        }

        // ② 进程枚举兜底（L0：区分 idle 与 offline；30s 一探，结果缓存复用）
        if now - self.last_probe_ms >= PROBE_INTERVAL_MS {
            self.probe_cache = probe_processes(&mut self.sys);
            self.last_probe_ms = now;
        }
        let (zcode_alive, claude_alive) = self.probe_cache;

        // ③ 采集已勾选 Agent 的会话与用量（设置页 agents_enabled：勾选才采集/监控/展示，
        //    不勾选则完全不处理；设置键不存在时默认全部启用——兼容升级与首次运行）
        let enabled: Option<std::collections::HashSet<String>> = self
            .store
            .get_setting("agents_enabled")
            .and_then(|raw| serde_json::from_str(&raw).ok());
        let is_enabled =
            |id: &str| match &enabled {
                Some(set) => set.contains(id),
                None => true,
            };

        let mut views: Vec<SessionView> = vec![];
        for ad in &self.adapters {
            let agent_id = ad.id();
            if !is_enabled(agent_id) {
                continue;
            }
            let info_list = match ad.scan_sessions() {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("[{agent_id}] 会话扫描失败（本轮按空处理）：{e:#}");
                    had_error = true;
                    vec![]
                }
            };
            // 增量用量入库 + 水位推进（水位带 60s 安全余量，R4）
            let watermark = self
                .store
                .get_watermark(agent_id)
                .saturating_sub(WATERMARK_MARGIN_MS);
            let usage = match ad.collect_usage(watermark) {
                Ok(u) => u,
                Err(e) => {
                    log::warn!("[{agent_id}] 用量采集失败（本轮按空处理）：{e:#}");
                    had_error = true;
                    vec![]
                }
            };
            self.store.insert_usage(&usage);
            if let Some(max_ts) = usage.iter().map(|u| u.ts).max() {
                self.store.set_watermark(agent_id, max_ts);
            }
            // 本轮新采集中的最近错误（按会话）
            let mut recent_errors: HashMap<&str, i64> = HashMap::new();
            for u in &usage {
                if u.error_type.is_some() && u.ts > *recent_errors.get(u.session_id.as_str()).unwrap_or(&0) {
                    recent_errors.insert(u.session_id.as_str(), u.ts);
                }
            }

            // 批量取本批会话的累计 token 与最近模型（审查 2.2.2:
            // 替代旧的每会话 2 次点查——100 会话即每 10s 200 次无索引扫描）
            let ids: Vec<String> = info_list.iter().map(|i| i.id.clone()).collect();
            let totals = self.store.session_usage_totals(&ids);
            let latest_models = self.store.latest_session_models(&ids);

            for info in &info_list {
                let raw_sid = info.id.split_once(':').map(|(_, s)| s).unwrap_or("");
                let mut sig = SessionSignals {
                    last_activity_at: info.last_usage_at,
                    // 进程名匹配是各 Agent 的私有知识：新适配器接入时在此补充映射
                    process_alive: match agent_id {
                        "zcode" => zcode_alive,
                        "claude-code" => claude_alive,
                        _ => false,
                    },
                    ..Default::default()
                };
                // hooks 信号仅 claude-code 有（事件里 session_id 为原始 id）
                if agent_id == "claude-code" {
                    if let Some((hook, ts, msg)) = last_hooks.get(raw_sid) {
                        sig.last_hook = Some((hook.clone(), *ts));
                        sig.notification_message = msg.clone();
                    }
                }
                if let Some(&ts) = recent_errors.get(info.id.as_str()) {
                    if now - ts <= ERROR_FRESH_MS {
                        sig.recent_error = Some((ts, "usage_error".into()));
                    }
                }
                let state = compute_state(&sig, now);
                // 会话元数据与状态入库（收缩态查询走内存快照，库做持久层）
                self.store.upsert_session(
                    &info.id, agent_id,
                    info.provider.as_deref(), info.model.as_deref(),
                    info.project_dir.as_deref(), info.title.as_deref(),
                    info.last_seen_at,
                    &serde_json::to_string(&state).unwrap_or_default().trim_matches('"').to_string(),
                    None,
                );
                views.push(SessionView {
                    id: info.id.clone(),
                    agent: agent_id.to_string(),
                    // Claude Code scan 阶段拿不到 model，从自库最近一次调用兜底回填（R1）
                    model: info.model.clone().or_else(|| latest_models.get(&info.id).cloned()),
                    project_dir: info.project_dir.clone(),
                    title: info.title.clone(),
                    state,
                    session_tokens: totals.get(&info.id).copied().unwrap_or(0),
                    last_activity_at: info.last_usage_at,
                });
            }
        }

        // ④ GLM 额度：按间隔刷新，失败降级（最近快照仍可用）。
        //    额度失败有独立降级语义（glm.rs 已留痕），不计入 degraded——
        //    degraded 表达"观测能力受损"，而非"外部接口抖动"
        if now - self.last_quota_fetch >= QUOTA_REFRESH_MS {
            if let Some(glm) = &self.glm {
                if let Ok(rows) = glm.fetch_quota() {
                    for r in &rows {
                        self.store.insert_quota(r);
                    }
                    self.last_quota_fetch = now;
                }
            }
        }

        // ⑤ 额度耗尽检测（5h 窗口 100%）：只产出快照级标志位，不改写会话状态——
        // 会话状态被统一改成 Error 会让贴边标签的分段全变一色，丢失 Agent 区分度；
        // 额度告警由前端表达（胶囊变红/标签红光/额度弧线红）
        let quotas = self.store.latest_quotas();
        let quota_exhausted = quotas
            .iter()
            .any(|q| q.provider == "glm" && q.window_kind == "5h" && q.used_percent >= Some(100.0));

        let island = aggregate(&views.iter().map(|v| v.state).collect::<Vec<_>>());
        let quota_views = quotas
            .into_iter()
            .map(|q| QuotaView { provider: q.provider, window_kind: q.window_kind, used_percent: q.used_percent, reset_at: q.reset_at })
            .collect();
        // degraded：连续多轮有采集源失败才置位；恢复即清零
        if had_error {
            self.fail_streak += 1;
        } else {
            self.fail_streak = 0;
        }
        let degraded = self.fail_streak >= DEGRADE_AFTER_TICKS;
        if degraded {
            log::warn!("采集源连续 {} 轮失败，快照标记 degraded", self.fail_streak);
        }
        IslandSnapshot {
            sessions: views,
            island,
            quotas: quota_views,
            quota_exhausted,
            degraded,
            generated_at: now,
        }
    }
}

/// 进程枚举：返回 （zcode 活着， claude 活着）。
/// sys 由调用方持有复用（审查 2.2.4：Windows 上全量刷新进程含命令行读取，开销可观）
fn probe_processes(sys: &mut sysinfo::System) -> (bool, bool) {
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut z = false;
    let mut c = false;
    for (_, proc) in sys.processes() {
        let n = proc.name().to_string_lossy().to_ascii_lowercase();
        if n.contains("zcode") {
            z = true;
        }
        // claude CLI 是 npm shim，真实进程名为 node.exe，名字不含 claude（R3），
        // 须查命令行；claude-menu（菜单工具）、本工具自身、hook-bridge（hook 桥的
        // node 进程，路径含 ".claude"，寿命 ≤2s）均不算，防止已退出的会话被误判存活；
        // 原生 exe 形态名字即命中
        let cmd = proc
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy())
            .collect::<String>()
            .to_ascii_lowercase();
        if (n.contains("claude") || cmd.contains("claude"))
            && !cmd.contains("claude-menu")
            && !cmd.contains("agenttrackerisland")
            && !cmd.contains("hook-bridge")
        {
            c = true;
        }
        if z && c {
            break;
        }
    }
    (z, c)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// e2e：本机真实数据跑两轮 tick（手动：cargo test -- --ignored test_real_aggregator）
    #[test]
    #[ignore]
    fn test_real_aggregator() {
        let mut db = std::env::temp_dir();
        db.push(format!("at-t7-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let store = Arc::new(Store::open(&db).unwrap());
        let mut agg = Aggregator::new(store.clone());

        let snap1 = agg.tick();
        println!(
            "tick1：会话 {} 个 | 岛 {:?} | 额度 {:?}",
            snap1.sessions.len(),
            snap1.island,
            snap1.quotas.iter().map(|q| format!("{}:{}%", q.window_kind, q.used_percent.unwrap_or(0.0) as i32)).collect::<Vec<_>>()
        );
        // 本机双 Agent 都在运行，必有会话；状态必须合法
        assert!(!snap1.sessions.is_empty(), "本机应有活跃会话");
        assert!(snap1.sessions.iter().any(|s| s.agent == "zcode"), "应含 zcode 会话");
        assert!(snap1.sessions.iter().any(|s| s.agent == "claude-code"), "应含 claude-code 会话");
        // ZCode 当前会话在写数据 → 应为 working 或 error（额度 100% 时标红）
        let zc = snap1.sessions.iter().find(|s| s.agent == "zcode").unwrap();
        println!("zcode 会话：state={:?} tokens={}", zc.state, zc.session_tokens);
        assert!(zc.session_tokens > 0, "本会话应有 token 统计");

        // 第二轮：水位增量幂等（会话 token 不变或仅微增）
        let snap2 = agg.tick();
        assert_eq!(snap2.sessions.len(), snap1.sessions.len());
        let _ = std::fs::remove_file(&db);
    }
}
