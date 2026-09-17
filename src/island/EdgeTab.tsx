/**
 * 贴边隐藏态标签（M1-6）：独立微 UI，非胶囊截取。
 * 设计：**颜色 = Agent 身份**（ZC 绿 / CC 橙，新 Agent 从色板稳定分配），
 * 等宽分段（每 Agent 一块，大小不编码信息），状态用亮度/动效表达——
 * 工作中=全亮慢呼吸 / 等待=快闪 / 出错=红圈描边+快闪 / 空闲=45% 暗淡 / 离线=近隐没；
 * 顶部底边与左右半圆外沿均有额度发丝线（长度/弧长 = 5h 已用百分比，颜色随档位）；
 * 悬停色块有提示（名字/状态/token）
 */
import type { IslandSnapshot, SessionState, Thresholds } from "../shared/types";
import { agentColor } from "../shared/types";
import { quotaLevel } from "./IslandBar";

/** 额度档位 → 弧线颜色（与发丝线/进度条档位色一致） */
const ARC_STROKE: Record<string, string> = {
  normal: "#34d399",
  warn: "#fbbf24",
  danger: "#f87171",
};

/** 按 Agent 聚合最严重状态，等宽分段（大小不编码信息） */
function agentSegments(snap: IslandSnapshot | null) {
  if (!snap) return [];
  const severity: Record<string, number> = {
    error: 4,
    waiting: 3,
    working: 2,
    idle: 1,
    offline: -1,
  };
  const worst = new Map<string, SessionState>();
  const tokens = new Map<string, number>();
  for (const s of snap.sessions) {
    tokens.set(s.agent, (tokens.get(s.agent) ?? 0) + s.session_tokens);
    const cur = worst.get(s.agent);
    if (cur === undefined || (severity[s.state] ?? 0) > (severity[cur] ?? 0)) {
      worst.set(s.agent, s.state);
    }
  }
  // token 降序（位置 = 用量排名，大者靠前），tooltip 保留 token 详情
  return [...worst.entries()]
    .map(([agent, state]) => ({
      agent,
      state,
      tokens: tokens.get(agent) ?? 0,
    }))
    .sort((a, b) => b.tokens - a.tokens || a.agent.localeCompare(b.agent));
}

export default function EdgeTab({
  edge,
  snap,
  thresholds,
}: {
  edge: string;
  snap: IslandSnapshot | null;
  thresholds: Thresholds;
}) {
  const error = snap?.island === "any_error" || snap?.quota_exhausted === true;
  const cls = `edge-tab edge-tab-${edge}${error ? " edge-error" : ""}`;
  const segments = agentSegments(snap);

  // 5h 额度：百分比与档位（发丝线/弧线填充用）
  const q5h = snap?.quotas.find(
    (q) => q.provider === "glm" && q.window_kind === "5h",
  );
  const pct = q5h?.used_percent;
  const level = pct != null ? quotaLevel(pct, thresholds.warn, thresholds.danger) : "normal";
  const pctClamped = pct != null ? Math.min(100, Math.max(0, pct)) : 0;

  // 顶部贴边：等宽横条，身份色填充，状态由亮度/动效表达
  if (edge !== "left" && edge !== "right") {
    return (
      <div className={cls} data-tauri-drag-region>
        {segments.map((seg) => (
          <span
            key={seg.agent}
            className={`edge-seg st-${seg.state}`}
            style={{ background: agentColor(seg.agent) }}
            data-tauri-drag-region
          />
        ))}
        {pct != null && (
          <span className="edge-quota-track">
            <span
              className={`edge-quota-fill fill-${level}`}
              style={{ width: `${pctClamped}%` }}
            />
          </span>
        )}
      </div>
    );
  }

  // 左右贴边：SVG 扇形（从圆心辐射，等角切分），颜色/状态语言与顶部一致
  const mirror = edge === "right";
  const cx = mirror ? 20 : 0;
  const total = segments.length || 1;
  let acc = -90; // 从正上方开始顺时针扫到正下方
  // SVG path 的 TS 类型不含自定义 data 属性，经展开传参附加拖拽区域标记
  const wedgeDrag = { "data-tauri-drag-region": "" };
  const wedges = segments.map((seg) => {
    const a0 = acc;
    const a1 = acc + (1 / total) * 180;
    acc = a1;
    return (
      <path
        {...wedgeDrag}
        key={seg.agent}
        className={`edge-wedge st-${seg.state}`}
        style={{ fill: agentColor(seg.agent) }}
        d={wedgePath(cx, 24, 20, 24, a0, a1, mirror)}
      />
    );
  });

  // 额度弧线：与容器轮廓同一条半椭圆曲线（同圆心同半径），描边一半被裁、
  // 留一半贴边——曲率与隐藏态边缘严格一致；弧长 ∝ 已用百分比
  const arcPath =
    edge === "left"
      ? "M 0 0 A 20 24 0 0 1 0 48"
      : "M 20 0 A 20 24 0 0 0 20 48";

  return (
    <div className={cls} data-tauri-drag-region>
      <svg className="edge-svg" viewBox="0 0 20 48" preserveAspectRatio="none">
        {wedges}
        {pct != null && (
          <>
            <path d={arcPath} pathLength={100} className="edge-arc-track" />
            <path
              d={arcPath}
              pathLength={100}
              className="edge-arc"
              strokeDasharray={`${pctClamped} 100`}
              stroke={ARC_STROKE[level] ?? ARC_STROKE.normal}
            />
          </>
        )}
      </svg>
    </div>
  );
}

/** 扇形楔形路径：圆心 （cx,cy），椭圆半径 rx/ry，角度 a0→a1（度，-90=正上，顺时针） */
function wedgePath(
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  a0: number,
  a1: number,
  mirror: boolean,
): string {
  const pt = (a: number) => {
    const r = (a * Math.PI) / 180;
    const dx = rx * Math.cos(r);
    const x = mirror ? cx - dx : cx + dx;
    return `${x.toFixed(2)} ${(cy + ry * Math.sin(r)).toFixed(2)}`;
  };
  const large = a1 - a0 > 180 ? 1 : 0;
  return `M ${cx} ${cy} L ${pt(a0)} A ${rx} ${ry} 0 ${large} ${mirror ? 0 : 1} ${pt(a1)} Z`;
}
