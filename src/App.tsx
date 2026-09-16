/**
 * 灵动岛入口:消费 island-snapshot 快照;hover 展开面板(窗口高度动态调整,
 * 宽度恒定避免锚点跳变);收缩/展开全程不抢焦点(红线⑤)
 */
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import IslandBar from "./island/IslandBar";
import Panel from "./island/Panel";
import type { IslandSnapshot } from "./shared/types";
import "./App.css";

/** 收缩/展开的窗口尺寸(逻辑像素,宽恒定) */
const COLLAPSED_H = 48;
const EXPANDED_H = 520;

function App() {
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

export default App;
