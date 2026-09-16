/**
 * 展开面板:会话卡片区(活跃优先,可滚动)+ GLM 额度区(双窗口进度条+倒计时)
 * 点击会话卡片的跳转行为由 T10 接入(onClick 预留)
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { IslandSnapshot, SessionView } from "../shared/types";
import { SESSION_META, fmtCountdown, fmtTokens } from "../shared/types";
import { quotaLevel } from "./IslandBar";

/** Agent 徽标文字 */
const AGENT_BADGE: Record<string, string> = {
  "claude-code": "CC",
  zcode: "ZC",
};

/** 排序:活跃状态(working/waiting/error)优先,其次按最近活动降序 */
const ACTIVE_FIRST: Record<string, number> = {
  error: 0,
  waiting: 1,
  working: 2,
  online: 3,
  idle: 4,
  offline: 5,
};

function sortSessions(list: SessionView[]): SessionView[] {
  return [...list].sort((a, b) => {
    const ra = ACTIVE_FIRST[a.state] ?? 9;
    const rb = ACTIVE_FIRST[b.state] ?? 9;
    if (ra !== rb) return ra - rb;
    return (b.last_activity_at ?? 0) - (a.last_activity_at ?? 0);
  });
}

function SessionCard({ s }: { s: SessionView }) {
  const meta = SESSION_META[s.state];
  const project = s.project_dir?.split(/[\\/]/).filter(Boolean).pop() ?? "";
  // 点击跳转:激活该会话对应的终端/IDE 窗口(T10;未命中静默失败)
  const focus = () => {
    invoke("focus_session", { sessionId: s.id }).catch(() => {});
  };
  return (
    <div className="card" data-session-id={s.id} onClick={focus}>
      <span className={`dot ${meta.dot}`} />
      <span className="card-badge">{AGENT_BADGE[s.agent] ?? "??"}</span>
      <span className="card-model">{s.model ?? "--"}</span>
      <span className="card-project" title={s.project_dir ?? ""}>
        {project || "--"}
      </span>
      <span className="card-state">{meta.label}</span>
      <span className="card-tokens">{fmtTokens(s.session_tokens)}</span>
    </div>
  );
}

function QuotaLine({
  label,
  usedPercent,
  resetAt,
}: {
  label: string;
  usedPercent: number | null;
  resetAt: number | null;
}) {
  // 倒计时本地每 30 秒推进一次(快照本身 10s 一刷)
  const [, force] = useState(0);
  useEffect(() => {
    const t = setInterval(() => force((n) => n + 1), 30_000);
    return () => clearInterval(t);
  }, []);
  const pct = usedPercent != null ? Math.min(100, Math.max(0, usedPercent)) : 0;
  const level = usedPercent != null ? quotaLevel(pct) : "normal";
  return (
    <div className="quota-line">
      <span className="quota-label">{label}</span>
      <div className="quota-bar">
        <div className={`quota-fill fill-${level}`} style={{ width: `${pct}%` }} />
      </div>
      <span className={`quota-pct text-${level}`}>{usedPercent != null ? `${Math.round(pct)}%` : "--"}</span>
      <span className="quota-reset">重置 {fmtCountdown(resetAt)}</span>
    </div>
  );
}

export default function Panel({ snap }: { snap: IslandSnapshot }) {
  const sessions = sortSessions(snap.sessions).slice(0, 30);
  const q5h = snap.quotas.find(
    (q) => q.provider === "glm" && q.window_kind === "5h",
  );
  const qWeek = snap.quotas.find(
    (q) => q.provider === "glm" && q.window_kind === "weekly",
  );
  return (
    <div className="panel">
      <div className="panel-title">
        会话 · {snap.sessions.length}
        <span className="panel-hint">点击卡片跳转对应窗口</span>
      </div>
      <div className="panel-sessions">
        {sessions.map((s) => (
          <SessionCard key={s.id} s={s} />
        ))}
        {sessions.length === 0 && (
          <div className="panel-empty">暂无会话记录</div>
        )}
      </div>
      <div className="panel-quota">
        <div className="panel-title">GLM Coding Plan</div>
        <QuotaLine label="5 小时" usedPercent={q5h?.used_percent ?? null} resetAt={q5h?.reset_at ?? null} />
        <QuotaLine label="每 周" usedPercent={qWeek?.used_percent ?? null} resetAt={qWeek?.reset_at ?? null} />
      </div>
    </div>
  );
}
