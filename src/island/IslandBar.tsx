/**
 * 收缩态胶囊：状态灯 + 摘要文案 + token 缩写；整条可拖拽。
 * 拖拽不用 data-tauri-drag-region：其注入脚本在首次按下即进入系统拖拽循环，
 * 单击的 mouseup 被系统吞掉、onClick 永远收不到（表现为"双击才点得动"）；
 * 这里改为手动判别——按下后位移超阈值才进入系统拖拽，未超阈值松手即单击
 */
import { useRef } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { IslandSnapshot, Thresholds } from "../shared/types";
import { fmtTokens } from "../shared/types";

/** 单击/拖拽判定的位移阈值（逻辑像素）：按下后移动超过该值才算拖拽 */
const DRAG_THRESHOLD_PX = 6;

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

/** 额度百分比对应的警示等级（阈值来自设置页，R5；静默提醒，不弹窗不出声） */
export function quotaLevel(
  pct: number,
  warn: number,
  danger: number,
): "normal" | "warn" | "danger" {
  if (pct >= danger) return "danger";
  if (pct >= warn) return "warn";
  return "normal";
}

export default function IslandBar({
  snap,
  thresholds,
  onToggle,
}: {
  snap: IslandSnapshot | null;
  thresholds: Thresholds;
  /** 悬停展开关闭时：点击胶囊切换信息卡片展开/收起 */
  onToggle?: () => void;
}) {
  // 额度耗尽时胶囊整体按出错态展示（会话状态本身保持真实值）
  const islandState = snap?.quota_exhausted ? "any_error" : snap?.island;
  const meta = snap ? ISLAND_META[islandState ?? "no_sessions"] : ISLAND_META.no_sessions;
  const working = snap?.sessions.filter((s) => s.state === "working").length ?? 0;
  const total = snap?.sessions.length ?? 0;
  const q5h = snap?.quotas.find(
    (q) => q.provider === "glm" && q.window_kind === "5h",
  );
  const q5hPct = q5h?.used_percent;
  const quotaCls =
    q5hPct != null ? ` quota-${quotaLevel(q5hPct, thresholds.warn, thresholds.danger)}` : "";

  // 按压追踪：起点坐标 + 是否已进入系统拖拽（进入后松手不算单击）
  const pressOrigin = useRef<{ x: number; y: number } | null>(null);
  const dragging = useRef(false);

  /** 按下：仅记录起点，不立即拖拽（为单击判定留出位移余量） */
  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    pressOrigin.current = { x: e.clientX, y: e.clientY };
    dragging.current = false;
  };

  /** 按住移动：位移超阈值 → 交给系统拖拽（此后鼠标事件由系统接管，贴靠仍由 Rust 评估） */
  const onMouseMove = (e: React.MouseEvent) => {
    const o = pressOrigin.current;
    if (!o || dragging.current) return;
    if (Math.hypot(e.clientX - o.x, e.clientY - o.y) > DRAG_THRESHOLD_PX) {
      dragging.current = true;
      void getCurrentWebviewWindow().startDragging();
    }
  };

  /** 松手：未进入系统拖拽 = 单击 → 切换信息卡片（仅悬停展开关闭时 onToggle 有值） */
  const onMouseUp = () => {
    const wasDragging = dragging.current;
    pressOrigin.current = null;
    dragging.current = false;
    if (!wasDragging) onToggle?.();
  };

  return (
    <div
      className={`island${onToggle ? " island-clickable" : ""}`}
      onMouseDown={onMouseDown}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
      // 按下后未达拖拽阈值就移出胶囊松手：清除按压态，防止落点处的 mouseup 误判为单击
      onMouseLeave={() => {
        pressOrigin.current = null;
        dragging.current = false;
      }}
    >
      <span className={`dot ${meta.dot}`} />
      <span className="island-text">
        {snap
          ? `${total} 会话${working > 0 ? ` · ${working} 工作中` : ""} · ${meta.label}${
              // 降级可见（审查 1.1）：采集源连续失败时明确提示，与"没有会话"区分
              snap.degraded ? " · 采集异常" : ""
            }`
          : "AgentTrackerIsland 启动中…"}
      </span>
      {q5hPct != null && (
        <span className={`island-quota${quotaCls}`}>
          5h {Math.round(q5hPct)}%
        </span>
      )}
      {snap && snap.sessions.length > 0 && (
        <span className="island-tokens">
          累计 {fmtTokens(snap.sessions.reduce((a, s) => a + s.session_tokens, 0))}
        </span>
      )}
    </div>
  );
}
