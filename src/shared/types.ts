/**
 * 与 Rust 侧 serde 输出保持一致的快照类型（state/service.rs）
 */

export type SessionState =
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
  /** GLM 5h 额度已耗尽（100%）：胶囊/标签据此变红，但会话状态保持真实值 */
  quota_exhausted: boolean;
  /** 采集源连续失败（2026-09-17 审查新增）：岛收缩态据此提示"采集异常" */
  degraded?: boolean;
  generated_at: number;
}

/** 额度提醒阈值（设置页存储，前端启动时加载；默认 80/95） */
export interface Thresholds {
  warn: number;
  danger: number;
}

/** 可监控的 Agent 定义（设置页选择项；implemented=false 表示适配器待开发，勾选暂不采集） */
export const AGENT_DEFS: {
  id: string;
  label: string;
  /** 系统默认身份色（用户可在设置页自定义） */
  color: string;
  implemented: boolean;
}[] = [
  { id: "zcode", label: "ZCode", color: "#34d399", implemented: true },
  { id: "claude-code", label: "Claude Code", color: "#f59e0b", implemented: true },
  { id: "codex", label: "Codex", color: "#38bdf8", implemented: false },
  { id: "claude-desktop", label: "Claude Desktop", color: "#a78bfa", implemented: false },
];

/** Agent 默认身份色（id → 颜色；可被用户自定义覆盖） */
export const AGENT_COLORS: Record<string, string> = Object.fromEntries(
  AGENT_DEFS.map((a) => [a.id, a.color]),
);

/** 用户自定义色注册表（运行时由 App 从设置注入；颜色 = 身份，状态用亮度/动效表达） */
let currentColors: Record<string, string> = {};

export function setAgentColors(colors: Record<string, string>) {
  currentColors = colors;
}

const FALLBACK_COLORS = ["#a78bfa", "#38bdf8", "#f472b6", "#facc15", "#4ade80", "#fb7185"];

/** 取 Agent 身份色：用户自定义 → 默认表 → 未知 Agent 按名字散列稳定分配 */
export function agentColor(agent: string): string {
  const custom = currentColors[agent];
  if (custom) return custom;
  const known = AGENT_COLORS[agent];
  if (known) return known;
  let h = 0;
  for (let i = 0; i < agent.length; i++) h = (h * 31 + agent.charCodeAt(i)) >>> 0;
  return FALLBACK_COLORS[h % FALLBACK_COLORS.length];
}

/** 会话状态 → 状态点 class 与中文标签 */
export const SESSION_META: Record<SessionState, { dot: string; label: string }> = {
  working: { dot: "dot-green breathe", label: "工作中" },
  idle: { dot: "dot-green", label: "空闲" },
  waiting: { dot: "dot-amber", label: "等待输入" },
  error: { dot: "dot-red pulse", label: "出错" },
  offline: { dot: "dot-gray", label: "离线" },
};

/** token 数值缩写（K/M） */
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
