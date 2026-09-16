/**
 * 应用入口:按窗口 URL hash 分流 —— #settings 渲染设置页(常规窗口),
 * 其余渲染灵动岛(透明窗口);岛消费 island-snapshot 快照,hover 展开面板
 */
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import IslandBar from "./island/IslandBar";
import Panel from "./island/Panel";
import Settings from "./Settings";
import type { IslandSnapshot, Thresholds } from "./shared/types";
import "./App.css";

/** 收缩/展开的窗口尺寸(逻辑像素,宽恒定) */
const COLLAPSED_H = 48;
const EXPANDED_H = 520;

/** 阈值默认值与脏数据防御(R5:设置页可配,启动时加载一次,重启生效) */
const DEFAULT_THRESHOLDS: Thresholds = { warn: 80, danger: 95 };

function sanitizeThresholds(warn: unknown, danger: unknown): Thresholds {
  const w = Number(warn);
  const d = Number(danger);
  const ok = (v: number) => Number.isFinite(v) && v > 0 && v <= 100;
  if (!ok(w) || !ok(d) || w >= d) return DEFAULT_THRESHOLDS;
  return { warn: w, danger: d };
}

function IslandApp() {
  const [snap, setSnap] = useState<IslandSnapshot | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [thresholds, setThresholds] = useState<Thresholds>(DEFAULT_THRESHOLDS);

  // 启动时读取提醒阈值(历史脏数据/读取失败均回退默认)
  useEffect(() => {
    invoke<Record<string, string>>("get_settings")
      .then((s) => setThresholds(sanitizeThresholds(s.threshold_warn, s.threshold_danger)))
      .catch(() => {});
  }, []);

  useEffect(() => {
    const unlisten = listen<IslandSnapshot>("island-snapshot", (e) =>
      setSnap(e.payload),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // hover 展开/收起:仅调高度;失败不致命(尺寸权限缺失时内容被裁剪但不崩溃)
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    win
      .setSize(new LogicalSize(480, expanded ? EXPANDED_H : COLLAPSED_H))
      .catch(() => {});
  }, [expanded]);

  return (
    <div
      className="root"
      onMouseEnter={() => setExpanded(true)}
      onMouseLeave={() => setExpanded(false)}
    >
      <IslandBar snap={snap} thresholds={thresholds} />
      {expanded && snap && (
        <div className="panel-wrap">
          <Panel snap={snap} thresholds={thresholds} />
        </div>
      )}
    </div>
  );
}

/** 按 URL hash 分流:设置窗口 / 灵动岛窗口 */
function App() {
  if (window.location.hash === "#settings") {
    return <Settings />;
  }
  return <IslandApp />;
}

export default App;
