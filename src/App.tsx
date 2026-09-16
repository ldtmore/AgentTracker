/**
 * 应用入口:按窗口 URL hash 分流 —— #settings 渲染设置页(常规窗口),
 * 其余渲染灵动岛(透明窗口);岛消费 island-snapshot 快照,hover 展开面板
 */
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import IslandBar from "./island/IslandBar";
import Panel from "./island/Panel";
import Settings from "./Settings";
import type { IslandSnapshot } from "./shared/types";
import "./App.css";

/** 收缩/展开的窗口尺寸(逻辑像素,宽恒定) */
const COLLAPSED_H = 48;
const EXPANDED_H = 520;

function IslandApp() {
  const [snap, setSnap] = useState<IslandSnapshot | null>(null);
  const [expanded, setExpanded] = useState(false);

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
      <IslandBar snap={snap} />
      {expanded && snap && (
        <div className="panel-wrap">
          <Panel snap={snap} />
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
