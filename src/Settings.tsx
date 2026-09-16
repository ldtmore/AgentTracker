/**
 * 设置页(T11):GLM 凭据 / 提醒阈值 / 数据清理周期 / hooks 开关 / 开机自启
 * 保存写入 app_settings;GLM 凭据与阈值在应用重启后生效(聚合器启动时读取)
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  const [msg, setMsg] = useState("");

  useEffect(() => {
    (async () => {
      try {
        const s = (await invoke("get_settings")) as Record<string, string>;
        if (s.glm_base) setGlmBase(s.glm_base);
        if (s.glm_token) setGlmToken(s.glm_token);
        if (s.threshold_warn) setWarn(s.threshold_warn);
        if (s.threshold_danger) setDanger(s.threshold_danger);
        if (s.cleanup_days) setCleanupDays(Number(s.cleanup_days));
        if (s.glm_token_source) setTokenFrom(s.glm_token_source);
        setHooksOn(await invoke("hooks_status"));
        setAutoStart(await invoke("autostart_get"));
      } catch {
        /* 加载失败保持默认值 */
      }
    })();
  }, []);

  const save = async () => {
    try {
      const kv: [string, string][] = [
        ["glm_base", glmBase],
        ["glm_token", glmToken],
        ["threshold_warn", warn],
        ["threshold_danger", danger],
        ["cleanup_days", String(cleanupDays)],
      ];
      for (const [k, v] of kv) await invoke("set_setting", { key: k, value: v });
      setMsg("已保存;凭据与阈值将在重启应用后生效");
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
