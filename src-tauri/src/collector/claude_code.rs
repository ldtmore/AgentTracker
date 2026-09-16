//! Claude Code 适配器:解析 `~\.claude\projects\**\*.jsonl` 转录文件。
//! 数据源勘察见 docs/01-RESEARCH.md §2;采集策略:
//!   文件级 mtime 过滤(旧文件必无新行)→ 行级时间过滤 → 幂等键去重入库。
//! Claude Code JSONL 的时间戳是 ISO 8601,需转 Unix 毫秒;usage 字段为 snake_case。

use std::path::PathBuf;

use super::{AgentAdapter, SessionInfo, provider_from_model};
use crate::store::UsageRow;

/// 转录根目录(%USERPROFILE%\.claude\projects)
fn projects_root() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")?;
    Some(PathBuf::from(home).join(".claude").join("projects"))
}

/// 单条 assistant 消息的 usage 结构(仅取我们关心的字段,未知字段忽略)
#[derive(serde::Deserialize, Clone)]
struct MessageUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens_details: Option<OutputDetails>,
}

#[derive(serde::Deserialize, Clone)]
struct OutputDetails {
    /// 思考 token 在 details 里(对应 ZCode 的 reasoning_tokens)
    thinking_tokens: Option<i64>,
}

#[derive(serde::Deserialize, Clone)]
struct MessageBody {
    /// API 消息唯一 id:JSONL 中同一消息会重复出现多行(流式写入/会话恢复),
    /// 必须按它去重,否则统计虚高约 3 倍(与 ccusage 同口径)
    id: Option<String>,
    model: Option<String>,
    usage: Option<MessageUsage>,
}

/// JSONL 行结构(宽松解析,字段缺失即跳过该行)
#[derive(serde::Deserialize)]
struct TranscriptLine {
    #[serde(rename = "type")]
    kind: String,
    message: Option<MessageBody>,
    timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    /// 请求 id:与 message.id 组成去重键(ccusage 同口径)
    /// 同消息多 requestId = 多次真实 API 调用(重试/恢复重发),各自计消耗
    #[serde(rename = "requestId")]
    request_id: Option<String>,
}

pub struct ClaudeCodeAdapter {
    root: PathBuf,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        Self {
            root: projects_root().unwrap_or_else(|| PathBuf::from("")),
        }
    }

    /// 遍历所有转录文件(projects/{项目编码目录}/{sessionId}.jsonl)
    fn transcript_files(&self) -> Vec<PathBuf> {
        let mut out = vec![];
        let Ok(dirs) = std::fs::read_dir(&self.root) else {
            return out; // Claude Code 未装:静默降级(红线④)
        };
        for d in dirs.filter_map(|x| x.ok()) {
            let Ok(files) = std::fs::read_dir(d.path()) else { continue };
            for f in files.filter_map(|x| x.ok()) {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    out.push(p);
                }
            }
        }
        out
    }

    /// 文件 mtime(Unix 毫秒);取不到返回 0
    fn mtime_ms(p: &PathBuf) -> i64 {
        p.metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// 从转录文件头部(≤8KB)提取首个带 cwd 的行,得到真实项目路径(R2)。
    /// 展示与窗口跳转匹配都依赖真实路径(编码目录名无法与窗口标题匹配);
    /// 头部无 cwd(罕见,如全是 summary 行)时由调用方退回编码目录名。
    fn first_cwd(path: &std::path::Path) -> Option<String> {
        use std::io::Read;
        let mut f = std::fs::File::open(path).ok()?;
        let mut buf = vec![0u8; 8192];
        let n = f.read(&mut buf).ok()?;
        let head = String::from_utf8_lossy(&buf[..n]);
        for line in head.lines() {
            let Ok(j) = serde_json::from_str::<serde_json::Value>(line) else {
                continue; // 头部截断的半行等,跳过
            };
            if let Some(cwd) = j.get("cwd").and_then(|c| c.as_str()) {
                if !cwd.is_empty() {
                    return Some(cwd.to_string());
                }
            }
        }
        None
    }
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    /// 会话发现:每个 jsonl 文件即一个会话;最近 90 天有修改的才纳入
    fn scan_sessions(&self) -> anyhow::Result<Vec<SessionInfo>> {
        let cutoff = chrono::Utc::now().timestamp_millis() - 90 * 24 * 3600 * 1000;
        let mut out = vec![];
        for f in self.transcript_files() {
            let mtime = Self::mtime_ms(&f);
            if mtime < cutoff {
                continue;
            }
            let session_id = f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if session_id.is_empty() {
                continue;
            }
            // 项目目录:优先转录行内真实 cwd(R2);取不到退回编码目录名(仅展示兜底)
            let project =
                Self::first_cwd(&f).or_else(|| {
                    f.parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                });
            out.push(SessionInfo {
                id: format!("claude-code:{session_id}"),
                agent: "claude-code".into(),
                provider: None, // 由 collect_usage 按实际模型回填,scan 阶段未知
                model: None,
                project_dir: project,
                title: None,
                first_seen_at: mtime,
                last_seen_at: mtime,
                last_usage_at: Some(mtime),
            });
        }
        // 最近修改在前,截断 100
        out.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at));
        out.truncate(100);
        Ok(out)
    }

    /// 水位增量:跳过 mtime 早于水位的文件,再逐行解析 assistant 消息按时间过滤;
    /// 按 message.id 全局去重(同 id 保留用量快照最大的一行)
    fn collect_usage(&self, watermark_ts: i64) -> anyhow::Result<Vec<UsageRow>> {
        let mut dedup: std::collections::HashMap<String, UsageRow> = std::collections::HashMap::new();
        for f in self.transcript_files() {
            if Self::mtime_ms(&f) <= watermark_ts {
                continue; // 文件未变,必无新行
            }
            let file_session = f
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let content = match std::fs::read_to_string(&f) {
                Ok(c) => c,
                Err(_) => continue, // 文件被轮转/占用:跳过本轮,下次再试
            };
            for line in content.lines() {
                let Ok(j) = serde_json::from_str::<TranscriptLine>(line) else {
                    continue; // 容忍坏行(红线:解析失败不阻塞)
                };
                if j.kind != "assistant" {
                    continue;
                }
                let Some(msg) = j.message else { continue };
                let (Some(model), Some(usage)) = (msg.model.clone(), msg.usage.clone()) else {
                    continue;
                };
                // "<synthetic>" 等本地合成消息非真实模型调用,不进统计
                if model.starts_with('<') {
                    continue;
                }
                // 无 message.id 的行无法去重,防御性跳过(实测数据中不存在)
                let Some(msg_id) = msg.id.clone() else { continue };
                // 去重键与 ccusage/better-ccusage 同口径:messageId+requestId 组合,
                // 缺 requestId 时退化为纯 messageId
                let dedup_key = match j.request_id.as_deref() {
                    Some(rid) if !rid.is_empty() => format!("{msg_id}:{rid}"),
                    _ => msg_id,
                };
                let Some(ts) = j.timestamp.as_deref().and_then(iso_to_ms) else { continue };
                if ts <= watermark_ts {
                    continue;
                }
                let sid = j.session_id.unwrap_or_else(|| file_session.clone());
                let provider = provider_from_model(&model);
                let row = UsageRow {
                    session_id: format!("claude-code:{sid}"),
                    agent: "claude-code".into(),
                    model,
                    provider,
                    ts,
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    reasoning_tokens: usage
                        .output_tokens_details
                        .and_then(|d| d.thinking_tokens),
                    cache_read_tokens: usage.cache_read_input_tokens,
                    cache_creation_tokens: usage.cache_creation_input_tokens,
                    duration_ms: None, // JSONL 无时长字段(ZCode 独有)
                    ttft_ms: None,
                    error_type: None,
                };
                // 同去重键多行:保留用量快照最大者(与文件遍历顺序无关)
                let total = row_total(&row);
                dedup.entry(dedup_key)
                    .and_modify(|old| {
                        if total > row_total(old) {
                            *old = row.clone();
                        }
                    })
                    .or_insert(row);
            }
        }
        let mut rows: Vec<UsageRow> = dedup.into_values().collect();
        rows.sort_by_key(|r| r.ts);
        Ok(rows)
    }
}

/// 行用量四项之和(去重时的比较口径)
fn row_total(r: &UsageRow) -> i64 {
    r.input_tokens.unwrap_or(0)
        + r.output_tokens.unwrap_or(0)
        + r.cache_read_tokens.unwrap_or(0)
        + r.cache_creation_tokens.unwrap_or(0)
}

/// ISO 8601(如 2026-09-16T06:46:19.159Z)→ Unix 毫秒;解析失败返回 None
fn iso_to_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// 当前 Unix 毫秒
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ===== hooks 安装/卸载(增强档,设置页一键装卸;02-DESIGN §4) =====

/// 桥脚本源码编译进二进制,安装时写出到家目录(单一已知位置,用户可审计)
const BRIDGE_SOURCE: &str = include_str!("../../hook-bridge/hook-bridge.js");
/// 注入标记(卸载时按此识别自家条目)
const BRIDGE_MARK: &str = "hook-bridge.js";
/// 覆盖状态机全部迁移的事件清单
const HOOK_EVENTS: &[&str] = &[
    "SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse",
    "Notification", "Stop", "SessionEnd",
];

fn claude_settings_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".claude").join("settings.json"))
}

fn bridge_script_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".claude").join("hooks").join("hook-bridge.js"))
}

/// 查询 hooks 是否已安装(settings.json 中存在自家注入条目)
pub fn hooks_installed() -> bool {
    let Some(path) = claude_settings_path() else { return false };
    if !path.exists() {
        return false;
    }
    std::fs::read_to_string(&path)
        .map(|raw| raw.contains(BRIDGE_MARK))
        .unwrap_or(false)
}

/// 安装:①桥脚本写出到 ~\.claude\hooks\hook-bridge.js;
/// ②settings.json 备份后合并注入 7 事件(防重复);返回注入条数
pub fn install_hooks() -> anyhow::Result<usize> {
    let bridge = bridge_script_path().ok_or_else(|| anyhow::anyhow!("无法定位用户目录"))?;
    if let Some(dir) = bridge.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&bridge, BRIDGE_SOURCE)?;
    let settings = claude_settings_path().ok_or_else(|| anyhow::anyhow!("无法定位 settings.json"))?;
    let cmd = format!("node \"{}\"", bridge.to_string_lossy().replace('\\', "/"));
    inject_into_settings(&settings, &cmd)
}

/// 卸载:移除全部自家注入条目(含空事件键清理);返回移除条数。
/// 桥脚本文件保留(重装免复制,且无副作用)
pub fn uninstall_hooks() -> anyhow::Result<usize> {
    let settings = claude_settings_path().ok_or_else(|| anyhow::anyhow!("无法定位 settings.json"))?;
    uninstall_from_settings(&settings)
}

/// settings.json 注入核心(独立函数便于用临时文件做单测)
fn inject_into_settings(path: &std::path::Path, command: &str) -> anyhow::Result<usize> {
    let raw = std::fs::read_to_string(path)?;
    let mut s: serde_json::Value = serde_json::from_str(&raw)?;
    // 备份(带时间戳,不覆盖历史备份)
    let bak = path.with_extension(format!("json.bak-at-{}", now_ms()));
    std::fs::write(&bak, &raw)?;
    // 确保 hooks 对象存在
    if s.get("hooks").and_then(|h| h.as_object()).is_none() {
        s["hooks"] = serde_json::json!({});
    }
    let hooks = s["hooks"].as_object_mut().unwrap();
    let mut injected = 0usize;
    for ev in HOOK_EVENTS {
        let entry = hooks.entry(ev.to_string()).or_insert(serde_json::json!([]));
        if !entry.is_array() {
            continue; // 用户配置了非数组结构:不碰,保守跳过
        }
        let already = entry.as_array().unwrap().iter().any(|g| {
            g["hooks"].as_array().map(|hs| hs.iter().any(|h| {
                h["command"].as_str().map(|c| c.contains(BRIDGE_MARK)).unwrap_or(false)
            })).unwrap_or(false)
        });
        if already {
            continue;
        }
        entry.as_array_mut().unwrap().push(serde_json::json!({
            "hooks": [{ "type": "command", "command": command, "timeout": 10, "async": true }]
        }));
        injected += 1;
    }
    // 原子写:临时文件+rename,避免写一半损坏(红线:绝不弄坏用户配置)
    let tmp = path.with_extension("json.at-tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&s)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(injected)
}

/// settings.json 卸载核心
fn uninstall_from_settings(path: &std::path::Path) -> anyhow::Result<usize> {
    let raw = std::fs::read_to_string(path)?;
    let mut s: serde_json::Value = serde_json::from_str(&raw)?;
    let Some(hooks) = s.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return Ok(0);
    };
    let mut removed = 0usize;
    for ev in hooks.keys().cloned().collect::<Vec<_>>() {
        if let Some(arr) = hooks.get_mut(&ev).and_then(|v| v.as_array_mut()) {
            let before = arr.len();
            arr.retain(|g| {
                !g["hooks"].as_array().map(|hs| hs.iter().any(|h| {
                    h["command"].as_str().map(|c| c.contains(BRIDGE_MARK)).unwrap_or(false)
                })).unwrap_or(false)
            });
            removed += before - arr.len();
            if arr.is_empty() {
                hooks.remove(&ev);
            }
        }
    }
    if s["hooks"].as_object().map(|o| o.is_empty()).unwrap_or(true) {
        s.as_object_mut().unwrap().remove("hooks");
    }
    let tmp = path.with_extension("json.at-tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&s)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// hooks 注入/卸载往返:临时 settings 文件,验证防重复与完整还原
    #[test]
    fn test_hooks_install_uninstall_roundtrip() {
        let dir = std::env::temp_dir().join(format!("at-t6-hooks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        // 模拟用户已有 hooks(与真实文件同构)+ 其他配置段
        std::fs::write(&settings, r#"{
          "statusLine": {"type": "command"},
          "permissions": {"defaultMode": "auto"},
          "hooks": {
            "Stop": [{"hooks": [{"type": "command", "command": "powershell.exe -File notify.ps1"}]}]
          }
        }"#).unwrap();

        // 注入:7 个事件(Stop 已存在→追加不覆盖)
        let n = inject_into_settings(&settings, "node \"C:/x/.claude/hooks/hook-bridge.js\"").unwrap();
        assert_eq!(n, 7);
        let s1: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(s1["hooks"]["Stop"].as_array().unwrap().len(), 2, "Stop 应追加而非覆盖");
        assert_eq!(s1["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(s1["statusLine"]["type"], "command", "其他配置不受影响");

        // 重复注入:防重复,0 条
        let n2 = inject_into_settings(&settings, "node \"C:/x/.claude/hooks/hook-bridge.js\"").unwrap();
        assert_eq!(n2, 0);

        // 卸载:回到与原文件等价(自家条目全清,用户 hooks 原样保留)
        let removed = uninstall_from_settings(&settings).unwrap();
        assert_eq!(removed, 7);
        let s2: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(s2["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(s2["hooks"].as_object().unwrap().contains_key("Stop"));
        assert!(!s2["hooks"].as_object().unwrap().contains_key("PreToolUse"));
        assert_eq!(s2["statusLine"]["type"], "command");
        // 备份文件存在
        assert!(dir.read_dir().unwrap().any(|f| f.unwrap().file_name().to_string_lossy().starts_with("settings.json.bak-at-")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R2:转录头部 cwd 提取(真实路径;含 summary 行/截断半行容错)
    #[test]
    fn test_first_cwd() {
        let dir = std::env::temp_dir().join(format!("at-r2-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.jsonl");
        std::fs::write(&file, concat!(
            r#"{"type":"summary","summary":"无 cwd 的行应跳过"}"#, "\n",
            r#"{"type":"user","cwd":"F:\\MyProjectRepository\\AgentTracker","timestamp":"2026-09-16T10:00:00.000Z"}"#, "\n",
            r#"{"type":"user","cwd":"F:\\另一个目录不应被选中"}"#, "\n"
        )).unwrap();
        assert_eq!(
            ClaudeCodeAdapter::first_cwd(&file).as_deref(),
            Some("F:\\MyProjectRepository\\AgentTracker")
        );
        // 全部无 cwd:None(调用方退回编码目录名)
        let f2 = dir.join("empty.jsonl");
        std::fs::write(&f2, r#"{"type":"summary"}"#).unwrap();
        assert_eq!(ClaudeCodeAdapter::first_cwd(&f2), None);
        // 文件不存在:None
        assert_eq!(ClaudeCodeAdapter::first_cwd(&dir.join("nope.jsonl")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 端到端(手动:cargo test -- --ignored test_real_hooks_e2e):
    /// 安装 hook-bridge → headless 触发真实 hook 链 → 事件文件落盘 → Rust 消费 → 卸载还原
    #[test]
    #[ignore]
    fn test_real_hooks_e2e() {
        // Drop 守卫:测试 panic 也会执行卸载,杜绝注入残留
        struct HooksGuard;
        impl Drop for HooksGuard {
            fn drop(&mut self) {
                let _ = uninstall_hooks();
            }
        }
        let _guard = HooksGuard;
        // 幂等清理可能的历史残留
        let _ = uninstall_hooks();

        let settings = claude_settings_path().unwrap();
        let before = std::fs::read_to_string(&settings).unwrap();

        // 1) 安装
        let n = install_hooks().unwrap();
        assert!(n >= 1, "至少注入 1 个事件");

        // 2) headless 触发(无凭据也会走 SessionStart/UserPromptSubmit/SessionEnd)
        // Windows 上 claude 是 npm 的 .cmd shim,须经 cmd /c 调用(Rust 不走 PATHEXT)
        let out = std::process::Command::new("cmd")
            .args(["/c", "claude", "-p", "ok"])
            .current_dir(dirs_home())
            .output();
        assert!(out.is_ok(), "claude CLI 应可执行");
        let stderr = String::from_utf8_lossy(&out.as_ref().unwrap().stderr);
        println!("claude stderr: {}", stderr.lines().take(2).collect::<Vec<_>>().join(" | "));
        // async hook 后台写入,给足落盘时间
        std::thread::sleep(std::time::Duration::from_secs(3));

        // 3) 事件文件落盘且可消费
        let evfile = crate::collector::hook_events::events_file_path().unwrap();
        let (events, offset) = crate::collector::hook_events::read_events(&evfile, 0);
        let fresh: Vec<_> = events.iter().filter(|e| e.session_id != "manual-test").collect();
        println!("捕获事件({} 条,偏移 {}): {:?}", fresh.len(), offset,
            fresh.iter().map(|e| e.hook.as_str()).collect::<Vec<_>>());
        assert!(!fresh.is_empty(), "事件文件应有真实 hook 记录");
        assert!(fresh.iter().all(|e| !e.session_id.is_empty()), "事件应带 session_id");

        // 4) 卸载后 settings 完整还原(Drop 守卫兜底,此处显式验证)
        let removed = uninstall_hooks().unwrap();
        assert!(removed >= 1);
        let after: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let before_v: serde_json::Value = serde_json::from_str(&before).unwrap();
        assert_eq!(after, before_v, "卸载后 settings.json 应与安装前语义等价");
    }

    fn dirs_home() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap())
    }

    /// 纯单测:构造临时转录文件验证解析/过滤/幂等键输入
    #[test]
    fn test_parse_transcript_lines() {
        let dir = std::env::temp_dir().join(format!("at-t4-{}", std::process::id()));
        let proj = dir.join("F--AgentTracker-test");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("sess-test-0001.jsonl");
        let lines = [
            // 普通用户行:应被忽略
            r#"{"type":"user","timestamp":"2026-09-15T10:00:00.000Z","sessionId":"sess-test-0001"}"#,
            // assistant 行:有效,glm 模型
            r#"{"type":"assistant","timestamp":"2026-09-15T10:00:01.000Z","sessionId":"sess-test-0001","message":{"id":"msg_a","model":"glm-5.3","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":200,"cache_creation_input_tokens":10,"output_tokens_details":{"thinking_tokens":5}}}}"#,
            // 同 message.id 的重复行(流式中途快照,用量更小):应被去重且保留大快照
            r#"{"type":"assistant","timestamp":"2026-09-15T10:00:01.000Z","sessionId":"sess-test-0001","message":{"id":"msg_a","model":"glm-5.3","usage":{"input_tokens":40,"output_tokens":20,"cache_read_input_tokens":80}}}"#,
            // 另一条独立消息
            r#"{"type":"assistant","timestamp":"2026-09-15T10:00:03.000Z","sessionId":"sess-test-0001","message":{"id":"msg_b","model":"glm-5.3","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":0}}}"#,
            // 缺 usage 的 assistant 行:忽略
            r#"{"type":"assistant","timestamp":"2026-09-15T10:00:02.000Z","message":{"model":"glm-5.3"}}"#,
            // 坏 JSON 行:忽略
            r#"{broken"#,
        ];
        std::fs::write(&file, lines.join("\n")).unwrap();

        let ad = ClaudeCodeAdapter { root: dir.clone() };
        let rows = ad.collect_usage(0).unwrap();
        // msg_a 去重后保留大快照 + msg_b:共 2 行
        assert_eq!(rows.len(), 2, "同 message.id 必须去重");
        assert!(rows.iter().all(|r| r.session_id == "claude-code:sess-test-0001"));
        assert!(rows.iter().all(|r| r.agent == "claude-code" && r.provider.as_deref() == Some("glm")));
        let big = rows.iter().find(|r| r.input_tokens == Some(100)).expect("应保留用量大的快照");
        assert_eq!(big.output_tokens, Some(50));
        assert_eq!(big.reasoning_tokens, Some(5));
        assert!(rows.iter().all(|r| r.ts > 1_700_000_000_000));

        // 时间水位:全部行早于该水位 → 0 行
        let rows2 = ad.collect_usage(1_800_000_000_000_000).unwrap();
        assert!(rows2.is_empty());
        // 但注意:该临时文件 mtime 是"现在",> 大水位?不——水位比较用文件 mtime <= watermark 跳过,
        // 1.8e15 是远未来,mtime(现在)< 水位 → 文件被跳过,结果一致为空,验证文件级过滤也生效
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 集成:本机真实转录库(手动:cargo test -- --ignored)
    #[test]
    #[ignore]
    fn test_real_cc_collect() {
        let ad = ClaudeCodeAdapter::new();
        let sessions = ad.scan_sessions().unwrap();
        assert!(!sessions.is_empty(), "本机应有活跃 Claude Code 会话");
        let usage = ad.collect_usage(0).unwrap();
        assert!(!usage.is_empty(), "本机应有历史用量");
        assert!(usage.iter().all(|u| u.session_id.starts_with("claude-code:")));
        assert!(usage
            .iter()
            .all(|u| u.model.to_ascii_lowercase().starts_with("glm")));
        // 水位增量:紧接的第二次采集应接近空(容忍正在写入的新消息,避免竞态误报)
        let max_ts = usage.iter().map(|u| u.ts).max().unwrap();
        let second = ad.collect_usage(max_ts).unwrap();
        assert!(second.len() <= 5, "水位增量应接近空,实际 {} 行(增长中的会话)", second.len());
    }

    /// A2 对账:全量分项汇总打印,与 `npx ccusage` 输出人工比对
    /// (手动:cargo test -- --ignored test_real_cc_reconcile -- --nocapture)
    #[test]
    #[ignore]
    fn test_real_cc_reconcile_totals() {
        let ad = ClaudeCodeAdapter::new();
        let usage = ad.collect_usage(0).unwrap();
        let sum = |f: fn(&UsageRow) -> Option<i64>| usage.iter().filter_map(f).sum::<i64>();
        println!("行数(assistant消息): {}", usage.len());
        println!("input:  {}", sum(|r| r.input_tokens));
        println!("output: {}", sum(|r| r.output_tokens));
        println!("cache_read: {}", sum(|r| r.cache_read_tokens));
        println!("cache_creation: {}", sum(|r| r.cache_creation_tokens));
        println!(
            "total:  {}",
            sum(|r| r.input_tokens) + sum(|r| r.output_tokens)
                + sum(|r| r.cache_read_tokens) + sum(|r| r.cache_creation_tokens)
        );
    }
}
