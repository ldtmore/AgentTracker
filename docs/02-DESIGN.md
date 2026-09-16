# AgentTracker 实现方案(02-DESIGN)

> 状态:**v1.0 定稿**(基于 [00-REQUIREMENTS v1.0](00-REQUIREMENTS.md) + [01-RESEARCH](01-RESEARCH.md))
> 定稿:2026-09-16 | 技术栈与架构经阶段 2 调研确认,实施期改动须回写本文档

## 1. 技术栈(定稿)

| 层    | 选型                                             |
| ---- | ---------------------------------------------- |
| 框架   | Tauri 2(stable)                                |
| 后端   | Rust(edition 2024)                             |
| 前端   | React 18 + TypeScript + Vite                   |
| 存储   | rusqlite(bundled)——自库读写 + ZCode 库只读            |
| 文件监听 | notify-rs                                      |
| 毛玻璃  | window-vibrancy(Acrylic,Win11;降级 Mica/DWM 半透明) |
| 窗口激活 | windows-rs(SetForegroundWindow)                |
| 分发   | M0 便携 zip(tauri build 产物);NSIS M1              |

## 2. 架构与模块

```
┌────────────────── WebView(React) ──────────────────┐
│ IslandApp:收缩态 │ 展开面板 │ 设置页(内嵌简单表单) │
└──────────────┬─────────────────────────────────────┘
        Tauri command(event 推送 state_changed / quota_updated)
┌──────────────┴─────────────────────────────────────┐
│ Rust 核心(后台线程)                                 │
│ ┌───────────── Agent 适配器(trait)─────────────┐   │
│ │ zcode.rs:只读 db.sqlite 水位采集+状态推断    │   │
│ │ claude_code.rs:JSONL 解析+hooks 事件消费     │   │
│ ├───────────── Provider 适配器(trait)─────────┤   │
│ │ glm.rs:Monitor API→quota_snapshots          │   │
│ ├───────────── 状态聚合器 ────────────────────┤   │
│ │ 三级融合+看门狗(5min)+error 判定→会话状态   │   │
│ └───────────── store(SQLite 自库)─────────────┘   │
└─────────────────────────────────────────────────────┘
```

### 2.1 AgentAdapter trait

```rust
pub trait AgentAdapter: Send {
    fn id(&self) -> &'static str;                       // "zcode" | "claude-code"
    fn scan(&self) -> Vec<SessionInfo>;                 // 发现会话+元数据(可增量)
    fn collect_usage(&self, watermarks: &Watermarks) -> Vec<UsageRecord>; // 增量采集
    fn watch(&self, tx: Sender<AgentEvent>);            // 注册实时事件源(notify/hooks)
}
```

**zcode.rs(调研定论:纯只读)**

- 打开 `~\.zcode\cli\db\db.sqlite`(readOnly),按 `model_usage.started_at` 水位增量拉取
- 列白名单读取(provider_id/model_id/agent/mode/task_type/status/started_at/completed_at/
  duration_ms/time_to_first_token_ms/input_tokens/output_tokens/reasoning_tokens/
  cache_creation_input_tokens/cache_read_input_tokens/error_type/retry_count/session_id/turn_id)
- 状态推断:工作=最近 usage 在 90s 内 或 session.time_updated 活跃;错误=最近记录 error_type 非空
- 会话标题/目录:join `session` 表

**claude_code.rs**

- 扫描 `~\.claude\projects\**\*.jsonl`,解析 assistant 消息的 `message.model` + `message.usage`
- 幂等键:(session_id, 消息时间戳, model)
- 增量:按文件 mtime + 文件内偏移量(watermark)
- hooks 事件(增强档):消费 hook-bridge 写的事件文件(协议见 §4)

### 2.2 ProviderAdapter trait

```rust
pub trait ProviderAdapter: Send {
    fn id(&self) -> &'static str;           // "glm"
    fn fetch_quota(&self) -> anyhow::Result<Vec<QuotaSnapshot>>; // 5h+weekly 两条
}
```

**glm.rs**:`GET {base}/api/monitor/usage/quota/limit`,Authorization 裸 key,5s 超时;
字段容错解析(usage/remaining/currentValue/total/percentage/nextResetTime);
刷新:5min 定时+手动+启动;失败降级显示最近快照+时间。base/key 来自设置
(默认读环境变量 ANTHROPIC_AUTH_TOKEN 与 ANTHROPIC_BASE_URL)。

### 2.3 状态聚合器

- 每会话状态机:offline → online → working ⇄ idle;waiting;error
- 事件优先级:hooks 事件 > 文件/usage 时间启发式 > 进程存在性
- 看门狗:working >5min 无新事件/新 usage → idle;error 不受看门狗影响
- error 判定:hooks Notification 含限流关键词(§7 正则)/ ZCode error_type 非空 / 额度快照=100%
- 聚合岛收缩态:任一会话 error→红;否则任一 working→呼吸绿;否则常亮绿;无会话→灰
- 静默提醒:额度 ≥阈值(80/95 可配)→ 收缩态额度文字变琥珀/红,不弹窗不出声

## 3. 自库 SQLite Schema(migrations/0001_init.sql)

```sql
CREATE TABLE sessions(
  id TEXT PRIMARY KEY,            -- "{agent}:{sessionId}"
  agent TEXT NOT NULL, provider TEXT, model TEXT,
  project_dir TEXT, title TEXT,
  first_seen_at INTEGER, last_seen_at INTEGER,
  state TEXT DEFAULT 'offline', state_reason TEXT
);
CREATE TABLE usage_records(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL, agent TEXT NOT NULL,
  model TEXT NOT NULL, provider TEXT,
  ts INTEGER NOT NULL,
  input_tokens INTEGER, output_tokens INTEGER,
  reasoning_tokens INTEGER, cache_read_tokens INTEGER, cache_creation_tokens INTEGER,
  duration_ms INTEGER, ttft_ms INTEGER,          -- ZCode 独有,Claude Code 置 NULL
  error_type TEXT,
  UNIQUE(agent, session_id, ts, model)
);
CREATE INDEX idx_usage_ts ON usage_records(ts);
CREATE TABLE quota_snapshots(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider TEXT NOT NULL, window_kind TEXT NOT NULL,   -- '5h'|'weekly'
  used_percent REAL, used_tokens INTEGER,
  reset_at INTEGER, fetched_at INTEGER NOT NULL
);
CREATE TABLE status_events(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  agent TEXT, session_id TEXT, hook TEXT, payload TEXT, ts INTEGER NOT NULL
);
CREATE TABLE watermarks(
  agent TEXT PRIMARY KEY, last_ts INTEGER, last_offset INTEGER
);
CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT);
```

数据清理(设置项):按 `usage_records.ts`/`quota_snapshots.fetched_at` 滚动删除,
周期:永不(默认)/3年/2年/1年/6月/3月/1月/1周。

## 4. hook-bridge 事件协议(Claude Code 增强档)

- 事件文件:`%LOCALAPPDATA%\AgentTracker\events\claude-code.jsonl`(append-only)
- 安装:设置页"启用精确状态"按钮→向 `~\.claude\settings.json` 的 hooks 注入
  SessionStart/UserPromptSubmit/PreToolUse/PostToolUse/Stop/Notification/SessionEnd
  各一条:`node "<appdir>\hook-bridge.js" <事件名>`(async, timeout 5s)
  注入采用**读取-合并-写回**且先备份 settings.json.bak;卸载=精确移除自己注入的条目
- hook-bridge.js:stdin 读 JSON → append `{"ts":...,"hook":...,"session_id":...,"model":...}` → 退出
  (不连端口不找进程;主程序死活无关——红线②)
- ⚠️ T6 开工第一步:先装诊断 hook 打印真实 stdin 字段(文档站不可达,以实测为准)

## 5. 灵动岛窗口规格(Tauri)

| 项         | 配置                                                                                         |
| --------- | ------------------------------------------------------------------------------------------ |
| 主窗 island | decorations:false, always_on_top, skip_taskbar, transparent, resizable:false, shadow:false |
| 位置        | 默认顶部居中(计算 workArea);拖拽后坐标存 app_settings                                                    |
| 效果        | window-vibrancy Acrylic(Win11 22H2+),Win10 降级纯半透明深色                                        |
| 收缩态       | 高~40px 胶囊:状态灯(8px 圆点)+ "AgentTracker"或聚合徽标 + GLM 额度%                                       |
| 展开态       | hover 展开(max-height 过渡):会话卡片区(每卡:状态点/Agent图标/模型/项目名/本会话 token)+ 额度区(5h/周双条+倒计时)            |
| 展开 focus  | non-activating:展开不调 set_focus,pointerLeave 收起(红线⑤)                                         |
| 托盘        | 右键:设置/暂停监控/退出;左键:显示/隐藏岛                                                                    |
| 状态灯       | working=呼吸绿(CSS animation);idle/done=常亮绿;waiting=琥珀;error=红(微脉冲);offline=灰                 |

## 6. 设置页(内嵌 WebView 路由 /settings)

GLM 平台(bigmodel/z.ai)+ key(密码框,存 app_settings,明文本地——与现有 CLI 同级安全);
提醒阈值(80/95);数据清理周期;hooks 安装/卸载;开机自启(默认关)。

## 7. 风险与对策(实施期)

| 风险                     | 对策                              |
| ---------------------- | ------------------------------- |
| ZCode Schema 随版本漂移     | 列白名单+未知列忽略;scan 失败静默降级为"仅进程监控"  |
| Claude Code JSONL 格式变化 | 同上;对账任务(A2)作为回归项                |
| GLM API 变更             | 集中 glm.rs;快照降级展示                |
| Acrylic 在部分驱动下闪烁       | 设置项可关毛玻璃,退纯色                    |
| settings.json 与其他工具竞争写 | 写前备份+原子写(temp+rename);冲突时提示用户手查 |
| hooks stdin 字段与预期不符    | T6 诊断 hook 实测先行                 |
