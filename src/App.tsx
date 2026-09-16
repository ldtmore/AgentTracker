/**
 * 灵动岛收缩态(M0 占位版,T9 做完整展开面板)
 * 消费后端 10s tick 广播的 island-snapshot 事件;整条区域可拖拽换位(坐标由后端记忆)
 */
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

// 与 Rust state/service.rs 的 serde 输出保持一致
type SessionState =
  | "online"
  | "working"
  | "idle"
  | "waiting"
  | "error"
  | "offline";

interface SessionView {
  id: string;
  agent: string;
  model: string | null;
  project_dir: string | null;
  title: string | null;
  state: SessionState;
  session_tokens: number;
  last_activity_at: number | null;
}

interface QuotaView {
  provider: string;
  window_kind: string;
  used_percent: number | null;
  reset_at: number | null;
}

interface IslandSnapshot {
  sessions: SessionView[];
  island:
    | "no_sessions"
    | "all_idle"
    | "any_working"
    | "any_waiting"
    | "any_error";
  quotas: QuotaView[];
  generated_at: number;
}

/** 岛收缩态 → 状态灯 class 与文案 */
const ISLAND_META: Record<
  IslandSnapshot["island"],
  { dot: string; label: string }
> = {
  no_sessions: { dot: "dot-gray", label: "AgentTracker · 未检测到会话" },
  all_idle: { dot: "dot-green", label: "空闲" },
  any_working: { dot: "dot-green breathe", label: "工作中" },
  any_waiting: { dot: "dot-amber", label: "等待输入" },
  any_error: { dot: "dot-red pulse", label: "出错 / 额度耗尽" },
};

/** token 数值缩写(K/M) */
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}K`;
  return String(n);
}

function App() {
  const [snap, setSnap] = useState<IslandSnapshot | null>(null);

  useEffect(() => {
    const unlisten = listen<IslandSnapshot>("island-snapshot", (e) =>
      setSnap(e.payload),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const meta = snap ? ISLAND_META[snap.island] : ISLAND_META.no_sessions;
  const working = snap?.sessions.filter((s) => s.state === "working").length ?? 0;
  const total = snap?.sessions.length ?? 0;
  const quota5h = snap?.quotas.find(
    (q) => q.provider === "glm" && q.window_kind === "5h",
  );
  const quotaText = quota5h?.used_percent != null ? ` · 5h ${Math.round(quota5h.used_percent)}%` : "";

  return (
    <div className="island" data-tauri-drag-region>
      <span className={`dot ${meta.dot}`} data-tauri-drag-region />
      <span className="island-text" data-tauri-drag-region>
        {snap
          ? `${total} 会话${working > 0 ? ` · ${working} 工作中` : ""} · ${meta.label}${quotaText}`
          : "AgentTracker 启动中…"}
      </span>
      {snap && snap.sessions.length > 0 && (
        <span className="island-tokens" data-tauri-drag-region>
          {fmtTokens(
            snap.sessions.reduce((a, s) => a + s.session_tokens, 0),
          )}
        </span>
      )}
    </div>
  );
}

export default App;
