//! 聚合服务:定时 tick,把采集器/事件/额度融合为岛快照(IslandSnapshot)。
//! 调度策略:每 tick(建议 10s)做采集+状态计算;GLM 额度每 5 分钟刷新一次,
//! 刷新失败降级显示最近快照(红线④)。T8/T9 由 Tauri 后台线程驱动并向前端广播。

use std::collections::HashMap;
use std::sync::Arc;

use crate::collector::claude_code::ClaudeCodeAdapter;
use crate::collector::hook_events;
use crate::collector::zcode::ZcodeAdapter;
use crate::collector::AgentAdapter;
use crate::provider::glm::GlmProvider;
use crate::provider::ProviderAdapter;
use crate::state::{aggregate, compute_state, IslandState, SessionSignals, SessionState};
use crate::store::Store;

/// 额度刷新间隔(毫秒)
const QUOTA_REFRESH_MS: i64 = 5 * 60 * 1000;
/// 错误信号的持续窗口(与 state 模块一致)
const ERROR_FRESH_MS: i64 = 10 * 60 * 1000;

/// 展示用会话视图(serde 给前端)
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

/// 岛快照:一次 tick 的完整产出
#[derive(Debug, Clone, serde::Serialize)]
pub struct IslandSnapshot {
    pub sessions: Vec<SessionView>,
    pub island: IslandState,
    pub quotas: Vec<QuotaView>,
    pub generated_at: i64,
}

/// 聚合器(有状态:水位/事件偏移/额度刷新计时)
pub struct Aggregator {
    store: Arc<Store>,
    zcode: ZcodeAdapter,
    cc: ClaudeCodeAdapter,
    /// hook 事件文件的消费偏移(持久化于 app_settings)
    hook_offset: u64,
    /// 上次额度刷新成功时间
    last_quota_fetch: i64,
    glm: Option<GlmProvider>,
}

impl Aggregator {
    pub fn new(store: Arc<Store>) -> Self {
        let hook_offset = store
            .get_setting("hook_events_offset")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Self {
            store,
            zcode: ZcodeAdapter::new(),
            cc: ClaudeCodeAdapter::new(),
            hook_offset,
            last_quota_fetch: 0,
            glm: GlmProvider::discover().map(|c| GlmProvider::new(&c.base, &c.token)),
        }
    }

    /// 执行一轮采集+融合,返回岛快照
    pub fn tick(&mut self) -> IslandSnapshot {
        let now = now_ms();

        // ① hooks 事件增量(claude-code;文件不存在=未安装增强档,静默降级)
        let mut last_hooks: HashMap<String, (String, i64, Option<String>)> = HashMap::new();
        if let Some(path) = hook_events::events_file_path() {
            let (events, new_off) = hook_events::read_events(&path, self.hook_offset);
            self.hook_offset = new_off;
            self.store.set_setting("hook_events_offset", &new_off.to_string());
            for ev in events {
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
        }

        // ② 进程枚举兜底(L0:区分 idle 与 offline)
        let (zcode_alive, claude_alive) = probe_processes();

        // ③ 采集两个 Agent 的会话与用量
        let mut views: Vec<SessionView> = vec![];
        for (agent_id, info_list) in [
            ("zcode", self.zcode.scan_sessions().unwrap_or_default()),
            ("claude-code", self.cc.scan_sessions().unwrap_or_default()),
        ] {
            let adapter: &dyn AgentAdapter = if agent_id == "zcode" {
                &self.zcode
            } else {
                &self.cc
            };
            // 增量用量入库 + 水位推进
            let watermark = self.store.get_watermark(agent_id);
            let usage = adapter.collect_usage(watermark).unwrap_or_default();
            self.store.insert_usage(&usage);
            if let Some(max_ts) = usage.iter().map(|u| u.ts).max() {
                self.store.set_watermark(agent_id, max_ts);
            }
            // 本轮新采集中的最近错误(按会话)
            let mut recent_errors: HashMap<&str, i64> = HashMap::new();
            for u in &usage {
                if u.error_type.is_some() && u.ts > *recent_errors.get(u.session_id.as_str()).unwrap_or(&0) {
                    recent_errors.insert(u.session_id.as_str(), u.ts);
                }
            }

            for info in &info_list {
                let raw_sid = info.id.split_once(':').map(|(_, s)| s).unwrap_or("");
                let mut sig = SessionSignals {
                    last_activity_at: info.last_usage_at,
                    process_alive: if agent_id == "zcode" { zcode_alive } else { claude_alive },
                    ..Default::default()
                };
                // hooks 信号仅 claude-code 有(事件里 session_id 为原始 id)
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
                // 会话元数据与状态入库(收缩态查询走内存快照,库做持久层)
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
                    model: info.model.clone(),
                    project_dir: info.project_dir.clone(),
                    title: info.title.clone(),
                    state,
                    session_tokens: self.store.session_usage_total(&info.id),
                    last_activity_at: info.last_usage_at,
                });
            }
        }

        // ④ GLM 额度:按间隔刷新,失败降级(最近快照仍可用)
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

        // ⑤ 额度耗尽信号回填 error(5h 窗口 100% 时,活跃会话标红)
        let quotas = self.store.latest_quotas();
        let exhausted = quotas
            .iter()
            .any(|q| q.provider == "glm" && q.window_kind == "5h" && q.used_percent >= Some(100.0));
        if exhausted {
            for v in views.iter_mut() {
                if matches!(v.state, SessionState::Working | SessionState::Idle | SessionState::Online) {
                    v.state = SessionState::Error;
                }
            }
        }

        let island = aggregate(&views.iter().map(|v| v.state).collect::<Vec<_>>());
        let quota_views = quotas
            .into_iter()
            .map(|q| QuotaView { provider: q.provider, window_kind: q.window_kind, used_percent: q.used_percent, reset_at: q.reset_at })
            .collect();
        IslandSnapshot {
            sessions: views,
            island,
            quotas: quota_views,
            generated_at: now,
        }
    }
}

/// 进程枚举:返回 (zcode 活着, claude 活着)
fn probe_processes() -> (bool, bool) {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut z = false;
    let mut c = false;
    for (_, proc) in sys.processes() {
        let n = proc.name().to_string_lossy().to_ascii_lowercase();
        if n.contains("zcode") {
            z = true;
        }
        if n.contains("claude") {
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

    /// e2e:本机真实数据跑两轮 tick(手动:cargo test -- --ignored test_real_aggregator)
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
            "tick1: 会话 {} 个 | 岛 {:?} | 额度 {:?}",
            snap1.sessions.len(),
            snap1.island,
            snap1.quotas.iter().map(|q| format!("{}:{}%", q.window_kind, q.used_percent.unwrap_or(0.0) as i32)).collect::<Vec<_>>()
        );
        // 本机双 Agent 都在运行,必有会话;状态必须合法
        assert!(!snap1.sessions.is_empty(), "本机应有活跃会话");
        assert!(snap1.sessions.iter().any(|s| s.agent == "zcode"), "应含 zcode 会话");
        assert!(snap1.sessions.iter().any(|s| s.agent == "claude-code"), "应含 claude-code 会话");
        // ZCode 当前会话在写数据 → 应为 working 或 error(额度 100% 时标红)
        let zc = snap1.sessions.iter().find(|s| s.agent == "zcode").unwrap();
        println!("zcode 会话: state={:?} tokens={}", zc.state, zc.session_tokens);
        assert!(zc.session_tokens > 0, "本会话应有 token 统计");

        // 第二轮:水位增量幂等(会话 token 不变或仅微增)
        let snap2 = agg.tick();
        assert_eq!(snap2.sessions.len(), snap1.sessions.len());
        let _ = std::fs::remove_file(&db);
    }
}
