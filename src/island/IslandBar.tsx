/**
 * 收缩态胶囊（2026-09-18 展示改造 C1-C7）：状态灯 + "现在时"主文案 + 额度 + 今日消耗。
 * 主文案只描述当下活跃（出错/等待/工作中），历史会话数降级进 tooltip；
 * 额度自动选最紧张的窗口（修掉"周快满胶囊却全绿"盲区）；token 改今日口径。
 * 整条可拖拽。拖拽不用 data-tauri-drag-region：其注入脚本在首次按下即进入系统
 * 拖拽循环，单击的 mouseup 被系统吞掉、onClick 永远收不到（表现为"双击才点得动"）；
 * 这里改为手动判别——按下后位移超阈值才进入系统拖拽，未超阈值松手即单击。
 *
 * 注意：胶囊窗口收缩态仅 48px 高，自绘 tooltip 会被窗口边界裁剪，
 * 故本组件一律用原生 title（OS 级渲染，可越出窗口边界）
 */
import { useRef } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { IslandSnapshot, Thresholds } from "../shared/types";
import { fmtCountdownCN, fmtTokens, tensestQuota, windowLabel } from "../shared/types";
import { WarnIcon } from "../shared/icons";

/** 单击/拖拽判定的位移阈值（逻辑像素）：按下后移动超过该值才算拖拽 */
const DRAG_THRESHOLD_PX = 6;

/** 状态灯样式与聚合状态说明（title 用，主文案不再重复状态词——C1） */
const ISLAND_META: Record<
  IslandSnapshot["island"],
  { dot: string; label: string }
> = {
  no_sessions: { dot: "dot-gray", label: "未检测到会话" },
  all_idle: { dot: "dot-green", label: "空闲" },
  any_working: { dot: "dot-green breathe", label: "有会话正在工作" },
  any_waiting: { dot: "dot-amber", label: "有会话等待输入" },
  any_error: { dot: "dot-red pulse", label: "有会话出错 / 额度耗尽" },
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

/** 主文案（C2"现在时"方案）：只列非零的活跃计数，按出错 > 等待 > 工作中排序；
 *  额度耗尽并入文案（与红灯语义对齐）；状态词全行只出现一次（修 C1 重复） */
function activeText(snap: IslandSnapshot): string {
  const count = (st: string) =>
    snap.sessions.filter((s) => s.state === st).length;
  const parts: string[] = [];
  const err = count("error");
  const waiting = count("waiting");
  const working = count("working");
  if (err > 0) parts.push(`${err} 出错`);
  if (waiting > 0) parts.push(`${waiting} 等输入`);
  if (working > 0) parts.push(`${working} 工作中`);
  // 额度耗尽但无会话出错：红灯由 quota_exhausted 驱动，文案同步说明，避免"红点+空闲"矛盾
  if (snap.quota_exhausted && err === 0) parts.push("额度耗尽");
  if (parts.length > 0) return parts.join(" · ");
  if (snap.sessions.length > 0) return `空闲 · ${snap.sessions.length} 个会话`;
  return "未检测到会话";
}

/** 额度段的 tooltip：双窗口明细一览（GLM Coding Plan；未来多供应商自动扩展） */
function quotaTitle(snap: IslandSnapshot): string {
  const lines = snap.quotas.map(
    (q) =>
      `${windowLabel(q.window_kind)} 已用 ${Math.round(q.used_percent ?? 0)}%${
        q.reset_at != null
          ? ` · 预计 ${fmtCountdownCN(q.reset_at)}后刷新恢复`
          : ""
      }`,
  );
  return ["GLM Coding Plan", ...lines].join("\n");
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

  // 额度段（C4/C6）：自动选最紧张窗口；未配置可见（降级必须可见，宪法红线④）
  const tense = snap ? tensestQuota(snap.quotas) : null;
  const quotaCls =
    tense?.used_percent != null
      ? ` quota-${quotaLevel(tense.used_percent, thresholds.warn, thresholds.danger)}`
      : "";

  // 今日消耗（C3）：账单口径（含缓存）；"累计"降级进 tooltip
  const allTime = snap?.sessions.reduce((a, s) => a + s.session_tokens, 0) ?? 0;
  const showTokens = snap != null && (snap.today_tokens > 0 || snap.sessions.length > 0);

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
      <span className={`dot ${meta.dot}`} title={meta.label} />
      <span className="island-text" title={snap ? `${meta.label}${snap.degraded ? "\n采集源连续失败，数据可能滞后" : ""}` : undefined}>
        {snap ? activeText(snap) : "智岛启动中…"}
        {
          // 降级可见（审查 1.1 + C7）：警示图标强化，与正常文案拉开视觉差
          snap?.degraded && (
            <span className="island-degraded">
              <WarnIcon /> 采集异常
            </span>
          )
        }
      </span>
      {
        // 额度段：未配置/暂不可用都以灰字可见（C6），不再整段消失
        snap != null &&
          (snap.glm_configured ? (
            tense?.used_percent != null ? (
              <span className={`island-quota${quotaCls}`} title={quotaTitle(snap)}>
                GLM {windowLabel(tense.window_kind)}·{Math.round(tense.used_percent)}%
              </span>
            ) : (
              <span className="island-quota quota-off" title={"GLM Coding Plan\n额度查询暂不可用，约 5 分钟后自动重试"}>
                额度 --
              </span>
            )
          ) : (
            <span className="island-quota quota-off" title={"尚未配置 GLM 凭据\n到设置页配置后可展示 5h/周额度用量"}>
              额度未配置
            </span>
          ))
      }
      {showTokens && (
        <span
          className="island-tokens"
          title={`今日 ${fmtTokens(snap!.today_tokens)}（${snap!.today_calls} 次调用）\n全部历史 ${fmtTokens(allTime)}\n口径：含缓存 token，与账单一致`}
        >
          今日 {fmtTokens(snap!.today_tokens)}
        </span>
      )}
    </div>
  );
}
