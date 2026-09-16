/**
 * 与 Rust 侧 serde 输出保持一致的快照类型(state/service.rs)
 */

export type SessionState =
  | "online"
  | "working"
  | "idle"
  | "waiting"
  | "error"
  | "offline";

export interface SessionView {
  id: string;
  agent: string;
  model: string | null;
  project_dir: string | null;
  title: string | null;
  state: SessionState;
  session_tokens: number;
  last_activity_at: number | null;
}

export interface QuotaView {
  provider: string;
  window_kind: string;
  used_percent: number | null;
  reset_at: number | null;
}

export type IslandStateName =
  | "no_sessions"
  | "all_idle"
  | "any_working"
  | "any_waiting"
  | "any_error";

export interface IslandSnapshot {
  sessions: SessionView[];
  island: IslandStateName;
  quotas: QuotaView[];
  generated_at: number;
}

/** 额度提醒阈值(设置页存储,前端启动时加载;默认 80/95) */
export interface Thresholds {
  warn: number;
  danger: number;
}

/** 会话状态 → 状态点 class 与中文标签 */
export const SESSION_META: Record<SessionState, { dot: string; label: string }> = {
  online: { dot: "dot-gray", label: "在线" },
  working: { dot: "dot-green breathe", label: "工作中" },
  idle: { dot: "dot-green", label: "空闲" },
  waiting: { dot: "dot-amber", label: "等待输入" },
  error: { dot: "dot-red pulse", label: "出错" },
  offline: { dot: "dot-gray", label: "离线" },
};

/** token 数值缩写(K/M) */
export function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}K`;
  return String(n);
}

/** 重置时间 → 剩余时长文案 */
export function fmtCountdown(resetAt: number | null): string {
  if (resetAt == null) return "--";
  const diff = resetAt - Date.now();
  if (diff <= 0) return "即将重置";
  const h = Math.floor(diff / 3_600_000);
  const m = Math.floor((diff % 3_600_000) / 60_000);
  if (h >= 24) return `${Math.floor(h / 24)}d${h % 24}h`;
  if (h > 0) return `${h}h${m}m`;
  return `${m}m`;
}
