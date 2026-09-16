//! 状态聚合器:把三条数据源(hooks 事件 > 活动启发式 > 进程枚举)融合为会话状态,
//! 并聚合出灵动岛收缩态。设计依据 docs/02-DESIGN.md §2.3。
//! 计算均为纯函数(便于全矩阵测试);服务调度见 service.rs。

pub mod service;

use serde::{Deserialize, Serialize};

/// 会话状态机(serde 序列化供前端使用)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    /// 工作中(呼吸绿)
    Working,
    /// 回合完成/空闲(常亮绿)
    Idle,
    /// 等待用户输入或批准(琥珀)
    Waiting,
    /// 出错:限流/额度耗尽/进程退出(红,不受看门狗影响)
    Error,
    /// 会话结束/进程不在
    Offline,
}

/// 灵动岛收缩态(聚合全部会话)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IslandState {
    /// 无任何会话(灰)
    NoSessions,
    /// 全部空闲(常亮绿)
    AllIdle,
    /// 任一会话工作中(呼吸绿)
    AnyWorking,
    /// 任一等待输入(琥珀)
    AnyWaiting,
    /// 任一出错(红,最高优先级)
    AnyError,
}

/// 限流/额度类错误关键词(来源:glm-quota-line 同款正则 + 常见补充,01-RESEARCH §7)
fn is_rate_limit_message(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("rate limit")
        || m.contains("too many requests")
        || m.contains("too frequent")
        || m.contains("frequency")
        || m.contains("限流")
        || m.contains("频率")
        || m.contains("过于频繁")
        || m.contains("稍后再试")
        || m.contains("额度")
}

/// 看门狗:working 状态无活动的最长容忍(毫秒),超时回落 idle
const WATCHDOG_MS: i64 = 5 * 60 * 1000;
/// hooks 事件的有效窗口(毫秒):窗口内事件驱动状态;
/// 必须大于看门狗窗口,否则看门狗分支不可达;超窗回落启发式
const HOOK_FRESH_MS: i64 = 30 * 60 * 1000;
/// 活动启发式窗口:最近 usage/文件写入在此窗口内视为工作中(毫秒)
const ACTIVITY_FRESH_MS: i64 = 90 * 1000;
/// 错误信号的持续窗口(毫秒):窗口外的历史错误不再标红(service 层复用)
pub(crate) const ERROR_FRESH_MS: i64 = 10 * 60 * 1000;

/// 单会话状态判定的输入信号(三级数据源的快照)
#[derive(Debug, Clone, Default)]
pub struct SessionSignals {
    /// 最近的 hook 事件(hook 名 + 时间戳毫秒);None = 未装 hooks 或无事件
    pub last_hook: Option<(String, i64)>,
    /// Notification 消息文本(用于限流判定)
    pub notification_message: Option<String>,
    /// 最近一次模型调用/文件活动时间(毫秒)
    pub last_activity_at: Option<i64>,
    /// ZCode model_usage 的最近 error_type(毫秒, 字符串)
    pub recent_error: Option<(i64, String)>,
    /// Agent 进程是否在运行(L0 兜底)
    pub process_alive: bool,
}

/// 计算单会话状态(纯函数;now 为当前毫秒)
///
/// 优先级:error(限流消息/最近错误)> hooks 事件 > 活动启发式 > 进程枚举。
/// 看门狗:hooks 给出 working 但超 WATCHDOG_MS 无任何新信号 → 回落 idle。
/// 注:额度耗尽(5h 100%)不改写会话状态(M1-6)——service 层产出快照级
/// quota_exhausted 标志,由前端驱动胶囊/贴边标签变红,会话状态保持真实值。
pub fn compute_state(sig: &SessionSignals, now: i64) -> SessionState {
    // ① error 判定(不受看门狗影响)
    if let Some(msg) = sig.notification_message.as_deref() {
        if is_rate_limit_message(msg) {
            return SessionState::Error;
        }
    }
    if let Some((ts, _)) = sig.recent_error {
        if now - ts <= ERROR_FRESH_MS {
            return SessionState::Error;
        }
    }

    let last_signal = sig
        .last_hook
        .as_ref()
        .map(|(_, ts)| *ts)
        .or(sig.last_activity_at)
        .unwrap_or(i64::MIN);
    let fresh = now.saturating_sub(last_signal) <= WATCHDOG_MS;

    // ② hooks 事件驱动(事件在有效窗口内才可信)
    if let Some((hook, ts)) = &sig.last_hook {
        if now - ts <= HOOK_FRESH_MS {
            return match hook.as_str() {
                "Notification" => SessionState::Waiting,
                "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "SessionStart" => {
                    if fresh { SessionState::Working } else { SessionState::Idle } // 看门狗
                }
                "Stop" => SessionState::Idle,
                "SessionEnd" => SessionState::Offline,
                _ => {
                    // 未知事件名:退到启发式
                    heuristic(sig, now)
                }
            };
        }
    }

    // ③ 无新鲜 hooks:活动启发式 + 进程枚举
    heuristic(sig, now)
}

/// 启发式:近 ACTIVITY_FRESH_MS 有活动 → working;否则进程在 → idle;都不在 → offline
fn heuristic(sig: &SessionSignals, now: i64) -> SessionState {
    if let Some(ts) = sig.last_activity_at {
        if now - ts <= ACTIVITY_FRESH_MS {
            return SessionState::Working;
        }
    }
    if sig.process_alive {
        SessionState::Idle
    } else {
        SessionState::Offline
    }
}

/// 聚合岛收缩态:Error > Waiting > Working > Idle;无会话 → NoSessions
pub fn aggregate(sessions: &[SessionState]) -> IslandState {
    if sessions.is_empty() {
        return IslandState::NoSessions;
    }
    if sessions.contains(&SessionState::Error) {
        IslandState::AnyError
    } else if sessions.contains(&SessionState::Waiting) {
        IslandState::AnyWaiting
    } else if sessions.contains(&SessionState::Working) {
        IslandState::AnyWorking
    } else {
        IslandState::AllIdle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000_000_000;

    fn sig() -> SessionSignals {
        SessionSignals::default()
    }

    #[test]
    fn test_error_judgments() {
        // 限流消息 → error
        let mut s = sig();
        s.last_hook = Some(("Notification".into(), NOW - 1000));
        s.notification_message = Some("You have hit a rate limit".into());
        assert_eq!(compute_state(&s, NOW), SessionState::Error);
        // 中文限流
        s.notification_message = Some("请求过于频繁,请稍后再试".into());
        assert_eq!(compute_state(&s, NOW), SessionState::Error);
        // 普通通知(非限流)→ waiting
        s.notification_message = Some("Claude needs your permission".into());
        assert_eq!(compute_state(&s, NOW), SessionState::Waiting);
        // 最近错误(ZCode error_type)→ error;窗口外不标红
        let mut s2 = sig();
        s2.recent_error = Some((NOW - 60_000, "api_error".into()));
        assert_eq!(compute_state(&s2, NOW), SessionState::Error);
        s2.recent_error = Some((NOW - ERROR_FRESH_MS - 1, "api_error".into()));
        assert_eq!(compute_state(&s2, NOW), SessionState::Offline);
    }

    #[test]
    fn test_hooks_driven_and_watchdog() {
        // hooks: PreToolUse 新鲜 → working
        let mut s = sig();
        s.last_hook = Some(("PreToolUse".into(), NOW - 1000));
        assert_eq!(compute_state(&s, NOW), SessionState::Working);
        // Stop → idle
        s.last_hook = Some(("Stop".into(), NOW - 1000));
        assert_eq!(compute_state(&s, NOW), SessionState::Idle);
        // SessionEnd → offline
        s.last_hook = Some(("SessionEnd".into(), NOW - 1000));
        assert_eq!(compute_state(&s, NOW), SessionState::Offline);
        // 看门狗:PreToolUse 后超 5 分钟无新信号 → idle
        s.last_hook = Some(("PreToolUse".into(), NOW - WATCHDOG_MS - 1));
        assert_eq!(compute_state(&s, NOW), SessionState::Idle);
    }

    #[test]
    fn test_heuristic_fallback() {
        // 无 hooks:近 90s 有活动 → working
        let mut s = sig();
        s.last_activity_at = Some(NOW - 30_000);
        assert_eq!(compute_state(&s, NOW), SessionState::Working);
        // 活动过时但进程在 → idle
        s.last_activity_at = Some(NOW - ACTIVITY_FRESH_MS - 1);
        s.process_alive = true;
        assert_eq!(compute_state(&s, NOW), SessionState::Idle);
        // 全无 → offline
        s.process_alive = false;
        assert_eq!(compute_state(&s, NOW), SessionState::Offline);
    }

    #[test]
    fn test_island_aggregate() {
        assert_eq!(aggregate(&[]), IslandState::NoSessions);
        assert_eq!(aggregate(&[SessionState::Idle]), IslandState::AllIdle);
        assert_eq!(
            aggregate(&[SessionState::Idle, SessionState::Working]),
            IslandState::AnyWorking
        );
        assert_eq!(
            aggregate(&[SessionState::Working, SessionState::Waiting]),
            IslandState::AnyWaiting
        );
        assert_eq!(
            aggregate(&[SessionState::Waiting, SessionState::Error]),
            IslandState::AnyError
        );
    }
}
