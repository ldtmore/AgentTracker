//! Agent 采集器模块：定义 AgentAdapter 抽象与公共数据结构。
//! 每个被监控的 Agent（Claude Code、ZCode……）实现一份，禁止 if-else 堆砌（宪法§三）。

use crate::store::UsageRow;

/// 会话元数据（scan 产物，供岛 UI 与状态聚合使用）
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// 自库主键："{agent}：{原始sessionId}"
    pub id: String,
    pub agent: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub project_dir: Option<String>,
    pub title: Option<String>,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    /// 最近一次模型调用时间（毫秒），状态聚合的启发式输入
    pub last_usage_at: Option<i64>,
}

/// Agent 适配器抽象：实现者只读不改目标 Agent 的任何数据（红线①）
///
/// M0 采集模型为"定时轮询水位增量"（外层调度器驱动），watch 实时事件源
/// 是 M1 优化项——见 docs/02-DESIGN.md §2.1 备注。
pub trait AgentAdapter: Send + Sync {
    /// Agent 标识：'zcode' | 'claude-code' | ...
    fn id(&self) -> &'static str;

    /// 发现会话（元数据，不含用量）
    fn scan_sessions(&self) -> anyhow::Result<Vec<SessionInfo>>;

    /// 增量采集用量：返回 started_at 严格大于 watermark 的调用记录
    fn collect_usage(&self, watermark_ts: i64) -> anyhow::Result<Vec<UsageRow>>;
}

pub mod claude_code;
pub mod hook_events;
pub mod zcode;

/// 由模型名推断供应商（启发式，小而够用）
/// ZCode 的 provider_id 是内部 UUID，直接映射不可读；后续可换配置表
pub fn provider_from_model(model: &str) -> Option<String> {
    let p = model.to_ascii_lowercase();
    if p.starts_with("glm") {
        Some("glm".into())
    } else if p.starts_with("claude") {
        Some("anthropic".into())
    } else if p.starts_with("gpt") || p.starts_with("o1") || p.starts_with("o3") || p.starts_with("o4") {
        Some("openai".into())
    } else if p.starts_with("gemini") {
        Some("google".into())
    } else if p.starts_with("deepseek") {
        Some("deepseek".into())
    } else if p.starts_with("kimi") || p.starts_with("moonshot") {
        Some("moonshot".into())
    } else if p.starts_with("qwen") {
        Some("alibaba".into())
    } else {
        None
    }
}
