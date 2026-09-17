/**
 * 应用入口：按窗口 URL hash 分流 —— #settings 渲染设置页（常规窗口），
 * #report 渲染报表页（常规窗口），其余渲染灵动岛（透明窗口）；
 * 岛消费 island-snapshot 快照，hover 展开面板
 */
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import IslandBar from "./island/IslandBar";
import Panel from "./island/Panel";
import EdgeTab from "./island/EdgeTab";
import Settings from "./Settings";
import { AGENT_DEFS, setAgentColors } from "./shared/types";
import { useTheme } from "./shared/theme";
import type { IslandSnapshot, Thresholds } from "./shared/types";
import "./App.css";

// 报表页 lazy 分割：echarts 只在报表窗口加载，岛窗口 bundle 不受影响（01-RESEARCH §9）
const Report = lazy(() => import("./report/Report"));

/** 岛自适应尺寸（Rust island_metrics：显示器逻辑宽 × 30%，夹取 380–800） */
interface IslandMetrics {
  width: number;
  collapsed_h: number;
  expanded_h: number;
}
const DEFAULT_METRICS: IslandMetrics = { width: 360, collapsed_h: 48, expanded_h: 520 };

/** 阈值默认值与脏数据防御（R5：设置页可配，启动时加载一次，重启生效） */
const DEFAULT_THRESHOLDS: Thresholds = { warn: 80, danger: 95 };

function sanitizeThresholds(warn: unknown, danger: unknown): Thresholds {
  const w = Number(warn);
  const d = Number(danger);
  const ok = (v: number) => Number.isFinite(v) && v > 0 && v <= 100;
  if (!ok(w) || !ok(d) || w >= d) return DEFAULT_THRESHOLDS;
  return { warn: w, danger: d };
}

function IslandApp() {
  // 主题应用与跟随（M1-4）：结果写 <html> 的 data-theme，CSS 变量自动切换；岛无需感知返回值
  useTheme();
  const [snap, setSnap] = useState<IslandSnapshot | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [thresholds, setThresholds] = useState<Thresholds>(DEFAULT_THRESHOLDS);
  // 贴边状态（Rust 端 island-dock 事件推送）：edge=none/top/left/right,hidden=是否滑出隐藏
  const [dock, setDock] = useState<{ edge: string; hidden: boolean }>({
    edge: "none",
    hidden: false,
  });
  const dockRef = useRef(dock);
  // 岛自适应尺寸（挂载时从 Rust 获取，与贴边几何共用同一公式）
  const [metrics, setMetrics] = useState<IslandMetrics>(DEFAULT_METRICS);
  // 悬停是否自动展开信息卡片（设置项 hover_expand；关闭时点击岛展开/收回）
  const [hoverCard, setHoverCard] = useState(true);
  // 监控的 Agent 列表（设置项 agents_enabled；岛内只展示所选 Agent）
  const [agents, setAgents] = useState<string[]>(AGENT_DEFS.map((a) => a.id));
  // 贴边隐藏态下"滑入完成后再展开"的定时器
  const enterTimer = useRef<number | undefined>(undefined);
  // 移出后滑出隐藏的宽限定时器（400ms 内回来则取消，防误触）
  const leaveTimer = useRef<number | undefined>(undefined);

  // 启动时读取岛尺寸/提醒阈值/悬停展开开关/贴边状态（读取失败用默认值）
  useEffect(() => {
    invoke<IslandMetrics>("island_metrics").then(setMetrics).catch(() => {});
    // 初始贴边状态必须主动拉取：启动恢复在 setup 阶段已把窗口滑出隐藏，
    // 早于本窗口事件监听建立，island-dock 事件收不到；不拉取会把隐藏态渲染成完整胶囊
    invoke<{ edge: string; hidden: boolean }>("island_dock_state")
      .then((d) => {
        dockRef.current = d;
        setDock(d);
      })
      .catch(() => {});
    invoke<Record<string, string>>("get_settings")
      .then((s) => {
        setThresholds(sanitizeThresholds(s.threshold_warn, s.threshold_danger));
        if (s.hover_expand !== undefined) setHoverCard(s.hover_expand !== "0");
        if (s.agents_enabled) {
          try {
            const list = JSON.parse(s.agents_enabled) as string[];
            if (Array.isArray(list)) setAgents(list); // 空数组 = 用户选择全部不监控，尊重之
          } catch {
            /* 解析失败用默认全选 */
          }
        }
        if (s.agent_colors) {
          try {
            const colors = JSON.parse(s.agent_colors) as Record<string, string>;
            if (colors && typeof colors === "object") setAgentColors(colors);
          } catch {
            /* 解析失败用默认色 */
          }
        }
      })
      .catch(() => {});
  }, []);

  // 设置页修改悬停展开开关后实时推送
  useEffect(() => {
    const un = listen<boolean>("hover-expand-changed", (e) =>
      setHoverCard(e.payload),
    );
    return () => {
      un.then((f) => f());
    };
  }, []);

  // 设置页修改监控 Agent 列表/颜色后实时推送
  useEffect(() => {
    const un = listen<{ agents: string[]; colors: Record<string, string> }>(
      "agents-changed",
      (e) => {
        setAgents(e.payload.agents);
        setAgentColors(e.payload.colors);
      },
    );
    return () => {
      un.then((f) => f());
    };
  }, []);

  // 订阅贴边状态（吸附/滑出/滑入时由 Rust 推送）
  useEffect(() => {
    const un = listen<{ edge: string; hidden: boolean }>("island-dock", (e) => {
      dockRef.current = e.payload;
      setDock(e.payload);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<IslandSnapshot>("island-snapshot", (e) =>
      setSnap(e.payload),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // hover 展开/收起：仅调高度；失败不致命（尺寸权限缺失时内容被裁剪但不崩溃）
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    win
      .setSize(
        new LogicalSize(metrics.width, expanded ? metrics.expanded_h : metrics.collapsed_h),
      )
      .catch(() => {});
  }, [expanded, metrics]);

  // 进入贴边隐藏态时自动收起面板（Rust 已把窗口缩回收缩态，保持渲染一致）
  useEffect(() => {
    if (dock.hidden && expanded) setExpanded(false);
  }, [dock.hidden, expanded]);

  const visibleSnap = filterSnap(snap, agents);

  return (
    <div
      className="root"
      onMouseEnter={() => {
        window.clearTimeout(leaveTimer.current);
        if (dockRef.current.hidden) {
          // 贴边隐藏态：先滑入显示独立标签→胶囊；悬停展开开启时滑入完成后再展开面板
          invoke("island_peek", { show: true }).catch(() => {});
          window.clearTimeout(enterTimer.current);
          if (hoverCard) {
            enterTimer.current = window.setTimeout(() => setExpanded(true), 260);
          }
        } else if (hoverCard) {
          setExpanded(true);
        }
      }}
      onMouseDown={() => {
        // 拖拽开始：取消在播滑动动画，防止程序化移动打断系统拖拽
        invoke("island_drag_start").catch(() => {});
      }}
      onMouseLeave={() => {
        window.clearTimeout(enterTimer.current);
        setExpanded(false);
        if (dockRef.current.edge !== "none") {
          // 400ms 宽限后滑出隐藏（期间重新进入则取消）
          window.clearTimeout(leaveTimer.current);
          leaveTimer.current = window.setTimeout(() => {
            invoke("island_peek", { show: false }).catch(() => {});
          }, 400);
        }
      }}
    >
      {dock.hidden && !expanded ? (
        // 贴边隐藏态：独立信息标签（非胶囊截取）；面板收起完成后才渲染，避免错位闪现
        <EdgeTab edge={dock.edge} snap={visibleSnap} thresholds={thresholds} />
      ) : (
        <>
          <IslandBar
            snap={visibleSnap}
            thresholds={thresholds}
            onToggle={hoverCard ? undefined : () => setExpanded((v) => !v)}
          />
          {expanded && visibleSnap && (
            <div className="panel-wrap">
              <Panel snap={visibleSnap} thresholds={thresholds} />
            </div>
          )}
        </>
      )}
    </div>
  );
}

/**
 * 按设置过滤要展示的 Agent。后端 Aggregator 已按 agents_enabled 跳过未勾选
 * Agent 的扫描与采集，此处过滤是展示层兜底（设置推送与快照到达存在竞态窗口）
 */
function filterSnap(
  snap: IslandSnapshot | null,
  agents: string[],
): IslandSnapshot | null {
  if (!snap) return null;
  return { ...snap, sessions: snap.sessions.filter((s) => agents.includes(s.agent)) };
}

/** 按 URL hash 分流：设置窗口 / 报表窗口 / 灵动岛窗口 */
function App() {
  if (window.location.hash === "#settings") {
    return <Settings />;
  }
  if (window.location.hash === "#report") {
    return (
      <Suspense fallback={null}>
        <Report />
      </Suspense>
    );
  }
  return <IslandApp />;
}

export default App;
