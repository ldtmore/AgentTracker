/**
 * 设置页(T11):GLM 凭据 / 提醒阈值 / 数据清理周期 / hooks 开关 / 开机自启
 * 保存写入 app_settings;GLM 凭据与阈值在应用重启后生效(聚合器启动时读取)。
 * Key 不回显;留空保存 = 沿用已存 Key 或自动发现链(env/claude-menu),不覆盖
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { AGENT_COLORS, AGENT_DEFS } from "./shared/types";
import "./settings.css";

/** 数据清理周期选项(天;0=永不清理) */
const CLEANUP_OPTIONS: { label: string; days: number }[] = [
  { label: "永不清理(默认)", days: 0 },
  { label: "保留 3 年", days: 1095 },
  { label: "保留 2 年", days: 730 },
  { label: "保留 1 年", days: 365 },
  { label: "保留 6 个月", days: 180 },
  { label: "保留 3 个月", days: 90 },
  { label: "保留 1 个月", days: 30 },
  { label: "保留 1 周", days: 7 },
];

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="st-section">
      <div className="st-title">{title}</div>
      {children}
    </div>
  );
}

export default function Settings() {
  const [glmBase, setGlmBase] = useState("https://open.bigmodel.cn");
  const [glmToken, setGlmToken] = useState("");
  const [tokenFrom, setTokenFrom] = useState("");
  const [warn, setWarn] = useState("80");
  const [danger, setDanger] = useState("95");
  const [cleanupDays, setCleanupDays] = useState(0);
  const [hooksOn, setHooksOn] = useState(false);
  const [autoStart, setAutoStart] = useState(false);
  // 灵动岛贴边自动隐藏(缺省=开,与 Rust 端 autohide_enabled 的默认一致)
  const [autoHide, setAutoHide] = useState(true);
  // 悬停自动展开信息卡片(缺省=开;关闭时点击岛展开/收回)
  const [hoverCard, setHoverCard] = useState(true);
  // 监控的 Agent 列表(缺省全选;勾选才采集/监控/展示,不勾选则不处理)
  const [agents, setAgents] = useState<string[]>(AGENT_DEFS.map((a) => a.id));
  // Agent 自定义身份色(未自定义的用系统默认色;隐藏态色块/面板徽标共用)
  const [agentColors, setAgentColors] = useState<Record<string, string>>({});
  const [msg, setMsg] = useState("");

  useEffect(() => {
    (async () => {
      try {
        const s = (await invoke("get_settings")) as Record<string, string>;
        if (s.glm_base) setGlmBase(s.glm_base);
        // Key 不回显:字段保持空,来源经 glm_token_source 提示(聚合器启动时写入)
        if (s.threshold_warn) setWarn(s.threshold_warn);
        if (s.threshold_danger) setDanger(s.threshold_danger);
        if (s.cleanup_days) setCleanupDays(Number(s.cleanup_days));
        if (s.glm_token_source) setTokenFrom(s.glm_token_source);
        if (s.island_autohide !== undefined) setAutoHide(s.island_autohide !== "0");
        if (s.hover_expand !== undefined) setHoverCard(s.hover_expand !== "0");
        if (s.agents_enabled) {
          try {
            const list = JSON.parse(s.agents_enabled) as string[];
            if (Array.isArray(list)) setAgents(list);
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
        setHooksOn(await invoke("hooks_status"));
        setAutoStart(await invoke("autostart_get"));
      } catch {
        /* 加载失败保持默认值 */
      }
    })();
  }, []);

  const save = async () => {
    // 阈值校验(R11):0–100 的数字,且琥珀阈值须小于红色阈值
    const w = Number(warn);
    const d = Number(danger);
    const inRange = (v: number) => Number.isFinite(v) && v > 0 && v <= 100;
    if (!inRange(w) || !inRange(d)) {
      setMsg("保存失败:阈值须为 0–100 的数字");
      return;
    }
    if (w >= d) {
      setMsg("保存失败:琥珀提醒阈值须小于红色告警阈值");
      return;
    }
    // Agent 身份色校验:已勾选的 Agent 之间颜色不得重复(颜色 = 身份)
    const checked = AGENT_DEFS.filter((a) => agents.includes(a.id));
    const effColor = (id: string) => agentColors[id] ?? AGENT_COLORS[id];
    const seen = new Map<string, string>();
    for (const a of checked) {
      const c = effColor(a.id).toLowerCase();
      if (seen.has(c)) {
        setMsg(`保存失败:${seen.get(c)} 与 ${a.label} 的颜色相同,不同 Agent 请使用不同颜色`);
        return;
      }
      seen.set(c, a.label);
    }
    try {
      // Key 留空 = 沿用已存 Key 或自动发现链,不写入空值覆盖(与提示文案一致)
      const kv: [string, string][] = [
        ["glm_base", glmBase],
        ["threshold_warn", warn],
        ["threshold_danger", danger],
        ["cleanup_days", String(cleanupDays)],
        ["island_autohide", autoHide ? "1" : "0"],
        ["hover_expand", hoverCard ? "1" : "0"],
        ["agents_enabled", JSON.stringify(agents)],
        ["agent_colors", JSON.stringify(agentColors)],
      ];
      if (glmToken) kv.splice(1, 0, ["glm_token", glmToken]);
      for (const [k, v] of kv) await invoke("set_setting", { key: k, value: v });
      // 贴边设置即时生效:如关闭自动隐藏时岛正处于隐藏态,Rust 会把它滑回显示
      await invoke("island_refresh").catch(() => {});
      // 悬停展开/监控 Agent(含颜色)实时推送给岛窗口
      await emit("hover-expand-changed", hoverCard).catch(() => {});
      await emit("agents-changed", { agents, colors: agentColors }).catch(() => {});
      setMsg("已保存;凭据与阈值将在重启应用后生效");
      // 保存成功自动关闭设置窗口(hide:托盘可再次唤起);稍作停留让提示可感知
      setTimeout(() => {
        getCurrentWebviewWindow().hide().catch(() => {});
      }, 600);
    } catch (e) {
      setMsg(`保存失败:${e}`);
    }
  };

  const toggleHooks = async () => {
    try {
      if (hooksOn) {
        await invoke("uninstall_hooks");
      } else {
        await invoke("install_hooks");
      }
      setHooksOn(await invoke("hooks_status"));
      setMsg(hooksOn ? "已停用精确状态(hooks 已卸载)" : "已启用精确状态(hooks 已注入)");
    } catch (e) {
      setMsg(`操作失败:${e}`);
    }
  };

  const toggleAutoStart = async (on: boolean) => {
    setAutoStart(on);
    try {
      await invoke("autostart_set", { enable: on });
    } catch (e) {
      setAutoStart(!on);
      setMsg(`设置失败:${e}`);
    }
  };

  return (
    <div className="st-root">
      <h2 className="st-header">AgentTracker 设置</h2>

      <Section title="GLM Coding Plan 凭据">
        <label className="st-label">平台</label>
        <select
          className="st-input"
          value={glmBase}
          onChange={(e) => setGlmBase(e.target.value)}
        >
          <option value="https://open.bigmodel.cn">智谱 BigModel(国内)</option>
          <option value="https://api.z.ai">Z.AI(国际)</option>
        </select>
        <label className="st-label">
          API Key{tokenFrom ? `(当前:${tokenFrom},留空则继续沿用)` : "(与 Claude Code 的 ANTHROPIC_AUTH_TOKEN 同值)"}
        </label>
        <input
          className="st-input"
          type="password"
          value={glmToken}
          placeholder="不回显;未配置时自动发现 claude-menu 配置"
          onChange={(e) => setGlmToken(e.target.value)}
        />
      </Section>

      <Section title="额度提醒阈值(%)">
        <div className="st-row">
          <div>
            <label className="st-label">琥珀提醒</label>
            <input className="st-input" type="number" value={warn} onChange={(e) => setWarn(e.target.value)} />
          </div>
          <div>
            <label className="st-label">红色告警</label>
            <input className="st-input" type="number" value={danger} onChange={(e) => setDanger(e.target.value)} />
          </div>
        </div>
      </Section>

      <Section title="统计数据保留">
        <select
          className="st-input"
          value={cleanupDays}
          onChange={(e) => setCleanupDays(Number(e.target.value))}
        >
          {CLEANUP_OPTIONS.map((o) => (
            <option key={o.days} value={o.days}>
              {o.label}
            </option>
          ))}
        </select>
        <div className="st-hint">应用启动时按周期清理历史用量/快照/事件</div>
      </Section>

      <Section title="Claude Code 精确状态(hooks)">
        <div className="st-row st-switch" onClick={toggleHooks}>
          <span>{hooksOn ? "已启用:实时精确状态(工作中/等待输入)" : "未启用:使用启发式状态(约 90 秒精度)"}</span>
          <button className="st-btn">{hooksOn ? "停用并卸载" : "启用(注入 hooks)"}</button>
        </div>
        <div className="st-hint">注入/卸载自动备份 settings.json;停用后 Claude Code 无任何感知</div>
      </Section>

      <Section title="灵动岛">
        <div className="st-row st-switch" onClick={() => setAutoHide(!autoHide)}>
          <span>
            {autoHide
              ? "贴边自动隐藏(默认):贴靠屏幕上/左/右边缘后滑出,仅露一点边缘,鼠标移入自动显示"
              : "已关闭:拖到边缘只吸附停靠,保持可见"}
          </span>
          <input type="checkbox" checked={autoHide} readOnly />
        </div>
        <div className="st-row st-switch" onClick={() => setHoverCard(!hoverCard)}>
          <span>
            {hoverCard
              ? "悬停自动展开(默认):鼠标移入岛即展开下方信息卡片"
              : "点击展开:点击岛展开信息卡片,再次点击收回;移出岛后卡片自动收起"}
          </span>
          <input type="checkbox" checked={hoverCard} readOnly />
        </div>
        <label className="st-label">选择 Agent 项(勾选才采集/监控/展示该 Agent 的数据)</label>
        <div className="st-agents">
          {AGENT_DEFS.map((a) => (
            <div key={a.id} className="st-agent">
              <label className="st-agent-check">
                <input
                  type="checkbox"
                  checked={agents.includes(a.id)}
                  onChange={(e) =>
                    setAgents((prev) =>
                      e.target.checked ? [...prev, a.id] : prev.filter((x) => x !== a.id),
                    )
                  }
                />
                {a.label}
                {!a.implemented && <span className="st-agent-todo">(适配器开发中)</span>}
              </label>
              <input
                type="color"
                className="st-agent-color"
                title="Agent 标识颜色"
                value={agentColors[a.id] ?? AGENT_COLORS[a.id]}
                onChange={(e) =>
                  setAgentColors((prev) => ({ ...prev, [a.id]: e.target.value }))
                }
              />
            </div>
          ))}
        </div>
        <div className="st-hint">
          颜色用于隐藏态分块与信息面板徽标,不同 Agent 请使用不同颜色;拖动灵动岛到屏幕边缘自动贴靠
        </div>
      </Section>

      <Section title="开机自启">
        <div className="st-row st-switch" onClick={() => toggleAutoStart(!autoStart)}>
          <span>{autoStart ? "开机自动启动" : "开机不启动(默认)"}</span>
          <input type="checkbox" checked={autoStart} readOnly />
        </div>
      </Section>

      <div className="st-footer">
        <span className="st-msg">{msg}</span>
        <button className="st-btn st-primary" onClick={save}>
          保存
        </button>
      </div>
    </div>
  );
}
