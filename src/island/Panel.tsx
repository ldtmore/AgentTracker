/**
 * 展开面板（2026-09-18 展示改造 P1-P11）：
 * 今日汇总条（活跃/今日消耗/调用次数 + 报表入口）
 * → 会话列表（活跃区全展示；历史区默认折叠为一行摘要，点击展开）
 * → GLM 额度区（双窗口进度条+倒计时）。
 * 卡片改两行：第一行 = 状态·相对时间 + 会话标题（主文案）；
 * 第二行 = 模型/项目徽章 + token（悬浮展示四项拆解）。
 * 点击会话卡片的跳转行为由 T10 接入（onClick 预留）
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { IslandSnapshot, SessionView, Thresholds } from "../shared/types";
import {
  SESSION_META,
  agentColor,
  errorReason,
  fmtCountdownCN,
  fmtRelative,
  fmtTokens,
} from "../shared/types";
import Tip from "../shared/Tip";
import { ChipIcon, FolderIcon, ReportIcon } from "../shared/icons";
import { quotaLevel } from "./IslandBar";

/** Agent 徽标文字 */
const AGENT_BADGE: Record<string, string> = {
  "claude-code": "CC",
  zcode: "ZC",
};

/** 排序：活跃状态（working/waiting/error）优先，其次按最近活动降序 */
const ACTIVE_FIRST: Record<string, number> = {
  error: 0,
  waiting: 1,
  working: 2,
  idle: 3,
  offline: 4,
};

/** 空闲超过该时长即按"已结束"展示（P5：历史会话≠空闲，进程级存活信号套在
 *  每个历史会话头上导致满屏假"空闲"——前端按时长近似修正，根治需会话级归属） */
const ENDED_AFTER_MS = 2 * 3_600_000;

/** 卡片展示态：真实状态 + 前端修正后的标签/状态灯 */
interface DisplayState {
  label: string;
  dot: string;
  ended: boolean;
}

function displayState(s: SessionView): DisplayState {
  // 活跃三态原样展示（working/waiting/error 不修正）
  if (s.state !== "idle" && s.state !== "offline") {
    return { ...SESSION_META[s.state], ended: false };
  }
  // offline = 进程已退出 → 会话已结束
  if (s.state === "offline") {
    return { label: "已结束", dot: "dot-gray", ended: true };
  }
  // idle：进程还开着；但超过 2 小时无活动按"已结束"展示（保留状态机真实值，仅改文案）
  const stale = s.last_activity_at != null && Date.now() - s.last_activity_at > ENDED_AFTER_MS;
  return stale
    ? { label: "已结束", dot: "dot-gray", ended: true }
    : { label: "空闲", dot: "dot-green", ended: false };
}

function sortSessions(list: SessionView[]): SessionView[] {
  return [...list].sort((a, b) => {
    const ra = ACTIVE_FIRST[a.state] ?? 9;
    const rb = ACTIVE_FIRST[b.state] ?? 9;
    if (ra !== rb) return ra - rb;
    return (b.last_activity_at ?? 0) - (a.last_activity_at ?? 0);
  });
}

/** 卡片主文案：标题优先，缺失回退项目名，再回退 id（P3/P9） */
function cardTitle(s: SessionView): string {
  return s.title ?? s.project_dir?.split(/[\\/]/).filter(Boolean).pop() ?? s.id;
}

function SessionCard({ s }: { s: SessionView }) {
  const disp = displayState(s);
  const project = s.project_dir?.split(/[\\/]/).filter(Boolean).pop() ?? "";
  const model = s.model;
  // 状态 + 相对时间（P4）：时间量感是"空闲/已结束"可读的关键
  const timeText = fmtRelative(s.last_activity_at);
  const stateText = timeText && !disp.ended ? `${disp.label} · ${timeText}` : disp.label;
  // 出错原因（P7）：error_type 已采集未用——透出具体错因而非干巴巴"出错"
  const stateFinal = s.state === "error" ? errorReason(s.error_type) : stateText;
  // 点击跳转：激活该会话对应的终端/IDE 窗口（T10；未命中静默失败）
  const focus = () => {
    invoke("focus_session", { sessionId: s.id }).catch(() => {});
  };
  return (
    <div className={`card${disp.ended ? " card-ended" : ""}`} data-session-id={s.id} onClick={focus}>
      <span className={`dot ${disp.dot}`} />
      {/* 徽标底色 = Agent 身份色（颜色即身份） */}
      <span className="card-badge" style={{ background: agentColor(s.agent) }}>
        {AGENT_BADGE[s.agent] ?? "??"}
      </span>
      <div className="card-main">
        <div className="card-line1">
          {/* 悬浮展示完整标题（截断有省略号暗示，气泡只作补充——G2 规则③） */}
          <Tip content={cardTitle(s)}>
            <span className="card-title">{cardTitle(s)}</span>
          </Tip>
          <span className={`card-state${s.state === "error" ? " text-error" : ""}`}>{stateFinal}</span>
        </div>
        <div className="card-line2">
          <Tip content={model ? `最近使用的模型：${model}` : "尚未捕获该会话的模型调用"}>
            <span className="card-tag">
              <ChipIcon />
              {model ?? "--"}
            </span>
          </Tip>
          <Tip content={s.project_dir ?? "无法识别工作目录"}>
            <span className="card-tag">
              <FolderIcon />
              {project || "--"}
            </span>
          </Tip>
          <Tip
            content={
              <div className="tip-breakdown">
                <div>
                  输入 {fmtTokens(s.input_tokens)} · 输出 {fmtTokens(s.output_tokens)}
                </div>
                <div>
                  缓存读 {fmtTokens(s.cache_read_tokens)} · 缓存写 {fmtTokens(s.cache_creation_tokens)}
                </div>
                <div className="tip-dim">本会话累计 · 含缓存，与账单同口径</div>
              </div>
            }
          >
            <span className="card-tokens">{fmtTokens(s.session_tokens)}</span>
          </Tip>
        </div>
      </div>
    </div>
  );
}

/** 历史区（P2）：默认折叠一行摘要，点击展开全部历史卡片 */
function HistorySection({ list }: { list: SessionView[] }) {
  const [open, setOpen] = useState(false); // 默认折叠（用户拍板）
  const latest = list[0];
  const latestText = latest ? `${cardTitle(latest)} · ${fmtRelative(latest.last_activity_at)}` : "";
  return (
    <>
      <Tip
        content={
          <div className="tip-breakdown">
            <div>已结束的历史会话，默认收起</div>
            <div className="tip-dim">点击展开 / 收起列表</div>
          </div>
        }
      >
        <button className="history-toggle" onClick={() => setOpen((v) => !v)}>
          <span className={`chevron${open ? " chevron-open" : ""}`}>▸</span>
          已结束 {list.length} 个
          {!open && latestText && <span className="history-latest">最近：{latestText}</span>}
        </button>
      </Tip>
      {open && (
        <>
          {list.map((s) => (
            <SessionCard key={s.id} s={s} />
          ))}
        </>
      )}
    </>
  );
}

/** 单个额度窗口 chip（2026-09-18 二次优化）：迷你条 + 百分比一行排布，
 *  重置时间等细节收敛进悬浮提示（官方语义见各窗口 tooltip 文案） */
function QuotaChip({
  label,
  fullName,
  usedPercent,
  resetAt,
  thresholds,
}: {
  /** 行内短标签：5h / 7d（官方窗口机制的最短写法） */
  label: string;
  /** 悬浮提示用的中文名 */
  fullName: string;
  usedPercent: number | null;
  resetAt: number | null;
  thresholds: Thresholds;
}) {
  const pct = usedPercent != null ? Math.min(100, Math.max(0, usedPercent)) : 0;
  const level =
    usedPercent != null
      ? quotaLevel(pct, thresholds.warn, thresholds.danger)
      : "normal";
  // 悬浮提示单行无歧义："5 小时额度：已用 2% · 预计 1 小时 34 分钟后重置"
  let status: string;
  if (usedPercent == null) {
    status = "暂无数据，约 5 分钟后自动重试";
  } else if (resetAt == null) {
    status = pct === 0 ? "窗口刚刷新，暂无用量" : `已用 ${Math.round(pct)}% · 重置时间待供应商返回`;
  } else {
    status = `已用 ${Math.round(pct)}% · 预计 ${fmtCountdownCN(resetAt)}后重置`;
  }
  return (
    <Tip content={`${fullName}：${status}`}>
      <span className="quota-chip">
        <span className="quota-label">{label}</span>
        <div className="quota-bar">
          <div className={`quota-fill fill-${level}`} style={{ width: `${pct}%` }} />
        </div>
        <span className={`quota-pct text-${level}`}>
          {usedPercent != null ? `${Math.round(pct)}%` : "--"}
        </span>
      </span>
    </Tip>
  );
}

/** 今日汇总条（P1）：面板首行回答"今天整体怎么样"，报表按钮直达报表窗口 */
function SummaryBar({ snap }: { snap: IslandSnapshot }) {
  const activeNow = snap.sessions.filter(
    (s) => s.state === "working" || s.state === "waiting" || s.state === "error",
  ).length;
  const openReport = () => {
    invoke("show_report_window").catch(() => {});
  };
  return (
    <div className="sum-bar">
      <Tip content={`今日 ${fmtTokens(snap.today_tokens)}（含缓存，账单口径）`}>
        <span className="sum-text">
          {snap.today_calls > 0 ? (
            <>
              活跃 <b>{activeNow}</b> · 今日 <b>{fmtTokens(snap.today_tokens)}</b> · {snap.today_calls} 次调用
            </>
          ) : (
            "今日暂无用量"
          )}
        </span>
      </Tip>
      <Tip content="打开报表窗口">
        <button className="sum-link" onClick={openReport}>
          <ReportIcon />
          报表
        </button>
      </Tip>
    </div>
  );
}

export default function Panel({
  snap,
  thresholds,
  onNaturalHeight,
}: {
  snap: IslandSnapshot;
  thresholds: Thresholds;
  /** 内容自然高度上报（App 据此调窗口高度，超出上限转会话区内部滚动） */
  onNaturalHeight?: (h: number) => void;
}) {
  // 相对时间每 30s 重渲染一次（快照 10s 一刷，本地再兜一层"刚刚→N 分钟前"的推进）
  const [, force] = useState(0);
  useEffect(() => {
    const t = setInterval(() => force((n) => n + 1), 30_000);
    return () => clearInterval(t);
  }, []);

  // 面板自然高度测量（窗口高度自适应的数据源，2026-09-18 展示改造）：
  // 面板被 max-height 钳制时，溢出被会话区内部滚动吸收，面板自身高度不再反映
  // 真实内容量，须用"会话区 scrollHeight − clientHeight"的溢出量补偿上报，
  // 否则内容变多时窗口停在原高不再生长
  const panelRef = useRef<HTMLDivElement>(null);
  const sessionsRef = useRef<HTMLDivElement>(null);
  const reportHeight = useCallback(() => {
    if (!onNaturalHeight) return;
    const panel = panelRef.current;
    const sessions = sessionsRef.current;
    if (!panel || !sessions) return;
    const overflow = Math.max(0, sessions.scrollHeight - sessions.clientHeight);
    onNaturalHeight(Math.round(panel.getBoundingClientRect().height) + overflow);
  }, [onNaturalHeight]);

  // 每次渲染后上报：快照更新、历史区开合、相对时间推进都可能改变内容量
  useLayoutEffect(() => {
    reportHeight();
  });

  // 窗口尺寸变化（钳制边界移动）不经过 React 渲染，用 ResizeObserver 兜住
  useEffect(() => {
    const panel = panelRef.current;
    const sessions = sessionsRef.current;
    if (!panel || !sessions) return;
    const ro = new ResizeObserver(reportHeight);
    ro.observe(panel);
    ro.observe(sessions);
    return () => ro.disconnect();
  }, [reportHeight]);

  // 活跃/历史分层（P2）：活跃区全展示，历史区折叠。
  // 每次渲染现算（几十个会话开销可忽略），保证 30s 定时刷新时"空闲→已结束"即时翻转
  const sorted = sortSessions(snap.sessions);
  const active = sorted.filter((s) => !displayState(s).ended);
  const history = sorted.filter((s) => displayState(s).ended);

  const q5h = snap.quotas.find(
    (q) => q.provider === "glm" && q.window_kind === "5h",
  );
  const qWeek = snap.quotas.find(
    (q) => q.provider === "glm" && q.window_kind === "weekly",
  );
  return (
    <div className="panel" ref={panelRef}>
      <SummaryBar snap={snap} />
      <div className="panel-title">
        会话 · {snap.sessions.length}
        <span className="panel-hint">点击卡片跳转对应窗口</span>
      </div>
      <div className="panel-sessions" ref={sessionsRef}>
        {active.map((s) => (
          <SessionCard key={s.id} s={s} />
        ))}
        {history.length > 0 && <HistorySection list={history} />}
        {snap.sessions.length === 0 && (
          <div className="panel-empty">暂无会话记录</div>
        )}
      </div>
      <div className="panel-quota">
        <div className="panel-title">GLM Coding Plan</div>
        <div className="quota-row">
          <QuotaChip
            label="5h"
            fullName="5 小时额度"
            usedPercent={q5h?.used_percent ?? null}
            resetAt={q5h?.reset_at ?? null}
            thresholds={thresholds}
          />
          <span className="quota-sep" />
          <QuotaChip
            label="7d"
            fullName="7 天额度"
            usedPercent={qWeek?.used_percent ?? null}
            resetAt={qWeek?.reset_at ?? null}
            thresholds={thresholds}
          />
        </div>
      </div>
    </div>
  );
}
