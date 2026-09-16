/**
 * 收缩态胶囊:状态灯 + 摘要文案 + token 缩写;整条可拖拽
 */
import type { IslandSnapshot } from "../shared/types";
import { fmtTokens } from "../shared/types";

const ISLAND_META: Record<
  IslandSnapshot["island"],
  { dot: string; label: string }
> = {
  no_sessions: { dot: "dot-gray", label: "未检测到会话" },
  all_idle: { dot: "dot-green", label: "空闲" },
  any_working: { dot: "dot-green breathe", label: "工作中" },
  any_waiting: { dot: "dot-amber", label: "等待输入" },
  any_error: { dot: "dot-red pulse", label: "出错 / 额度耗尽" },
};

/** 额度百分比对应的警示等级(静默提醒:80% 琥珀 / 95% 红,M0 固定阈值) */
export function quotaLevel(pct: number): "normal" | "warn" | "danger" {
  if (pct >= 95) return "danger";
  if (pct >= 80) return "warn";
  return "normal";
}

export default function IslandBar({ snap }: { snap: IslandSnapshot | null }) {
  const meta = snap ? ISLAND_META[snap.island] : ISLAND_META.no_sessions;
  const working = snap?.sessions.filter((s) => s.state === "working").length ?? 0;
  const total = snap?.sessions.length ?? 0;
  const q5h = snap?.quotas.find(
    (q) => q.provider === "glm" && q.window_kind === "5h",
  );
  const q5hPct = q5h?.used_percent;
  const quotaText =
    q5hPct != null ? ` · 5h ${Math.round(q5hPct)}%` : "";
  const quotaCls =
    q5hPct != null ? ` quota-${quotaLevel(q5hPct)}` : "";

  return (
    <div className="island" data-tauri-drag-region>
      <span className={`dot ${meta.dot}`} data-tauri-drag-region />
      <span className="island-text" data-tauri-drag-region>
        {snap
          ? `${total} 会话${working > 0 ? ` · ${working} 工作中` : ""} · ${meta.label}`
          : "AgentTracker 启动中…"}
      </span>
      {q5hPct != null && (
        <span className={`island-quota${quotaCls}`} data-tauri-drag-region>
          5h {Math.round(q5hPct)}%
        </span>
      )}
      {snap && snap.sessions.length > 0 && (
        <span className="island-tokens" data-tauri-drag-region>
          {fmtTokens(snap.sessions.reduce((a, s) => a + s.session_tokens, 0))}
        </span>
      )}
    </div>
  );
}
