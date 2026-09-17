# AgentTrackerIsland 实现方案(02-DESIGN)

> 状态:**v1.0 定稿,回写至 2026-09-17**(基于 [00-REQUIREMENTS v1.0](00-REQUIREMENTS.md) + [01-RESEARCH](01-RESEARCH.md))
> 定稿:2026-09-16 | 技术栈与架构经阶段 2 调研确认,实施期改动须回写本文档

## 1. 技术栈(定稿,含落地回写)

| 层    | 选型                                             |
| ---- | ---------------------------------------------- |
| 框架   | Tauri 2(stable)                                |
| 后端   | Rust(edition 2021,Cargo.toml 实际值)             |
| 前端   | React 19 + TypeScript + Vite                   |
| 存储   | rusqlite(bundled)——自库读写 + ZCode 库只读            |
| 采集调度 | 定时轮询水位增量(10s tick;notify-rs 文件监听未引入,M1 视需要评估) |
| 窗口效果 | 窗口全透明 + CSS 自绘背景(window-vibrancy/Acrylic 已弃用——窗口级效果会把整个矩形染灰破坏胶囊形态,T8 定论;依赖保留备 M1 全宽形态) |
| 窗口激活 | windows-rs(SetForegroundWindow)                |
| 分发   | 便携 zip(release 产物)——仅所有者宣布正式发布时执行(NSIS 同,见 WORKFLOW 构建打包纪律) |

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

### 2.1 AgentAdapter trait(落地签名)

```rust
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &'static str;                       // "zcode" | "claude-code"
    fn scan_sessions(&self) -> anyhow::Result<Vec<SessionInfo>>;          // 发现会话
    fn collect_usage(&self, watermark_ts: i64) -> anyhow::Result<Vec<UsageRow>>; // 水位增量
}
```

> 📌 回写(2026-09-17):原设计的实时事件源 `watch()` 未实现——M0 采集模型为
> "定时轮询水位增量"(聚合器 10s tick 驱动),watch 推迟为 M1 优化项
> (见 src-tauri/src/collector/mod.rs 头注)。

**zcode.rs(调研定论:纯只读)**

- 打开 `~\.zcode\cli\db\db.sqlite`(readOnly),按 `model_usage.started_at` 水位增量拉取
- 列白名单读取(provider_id/model_id/agent/mode/task_type/status/started_at/completed_at/
  duration_ms/time_to_first_token_ms/input_tokens/output_tokens/reasoning_tokens/
  cache_creation_input_tokens/cache_read_input_tokens/error_type/retry_count/session_id/turn_id)
- 状态推断:工作=最近 usage 在 90s 内 或 session.time_updated 活跃;错误=最近记录 error_type 非空
- 会话标题/目录:join `session` 表

**claude_code.rs**

- 扫描 `~\.claude\projects\**\*.jsonl`,解析 assistant 消息的 `message.model` + `message.usage`
- 幂等键:(session_id, 消息时间戳, model);同键冲突时仅当新行四项用量合计更大才
  整行覆盖(保留最大快照,与流式去重同口径,2026-09-17 回写)
- 增量:按文件 mtime 过滤(≤水位跳过)+ 全量重读逐行时间过滤;"文件内偏移量"
  未实现(每 tick 重读有变动的文件,M0 规模可接受,列入看板待议区)
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
刷新:5min 定时+启动;失败降级显示最近快照。base/key 来自设置
(默认读环境变量 ANTHROPIC_AUTH_TOKEN 与 ANTHROPIC_BASE_URL)。
> 📌 M0 落地回写(2026-09-17):刷新实现为"定时 5min + 启动即拉"两路,"手动刷新"
> 未做(5min 粒度自用足够,所有者 2026-09-17 拍板),M1 报表页一并考虑。
> 📌 M1 回写(2026-09-17,审查修复):凭据优先级实现为"应用设置(token 非空才
> 生效)> 环境变量 > claude-menu suppliers.json"——设置 Key 留空不写入、自动回落
> 发现链(兑现设置页"留空则继续沿用");凭据来源写入 app_settings.glm_token_source
> 供设置页展示。百分比兜底仅接受 usage/remaining 换算,不可换算返回 None
> (UI 显示 "--"),不拿 currentValue 绝对量冒充百分比。

### 2.3 状态聚合器

- 每会话状态机:working ⇄ idle;waiting;error;offline(原设计的 online 态
  无产出路径,M1-7 移除)
- 事件优先级:hooks 事件 > 文件/usage 时间启发式 > 进程存在性
- 看门狗:working >5min 无新事件/新 usage → idle;error 不受看门狗影响
- error 判定:hooks Notification 含限流关键词(§7 正则)/ ZCode error_type 非空
  > 📌 M0 落地回写(2026-09-17):卡片仅显示"出错"态,不展示原因明细;`state_reason`
  > 字段已预留,DB 写 NULL。原因诊断(额度耗尽/进程退出/API 报错)M1 诊断页一并做。
  > 📌 M1-6 回写(2026-09-17):"额度快照=100%"不再写入 error 判定/会话状态——
  > service 层产出快照级 quota_exhausted 标志,前端驱动胶囊变红/贴边标签红光/
  > 额度弧线红,会话状态保持真实值。
- 聚合岛收缩态:任一会话 error→红;否则任一 waiting→琥珀;否则任一 working→
  呼吸绿;否则常亮绿;无会话→灰
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

数据清理(设置项):按 `usage_records.ts`/`quota_snapshots.fetched_at`/
`status_events.ts` 滚动删除(启动时按周期执行一次),
周期:永不(默认)/3年/2年/1年/6月/3月/1月/1周。
`sessions` 表不在清理范围(每会话一行,增长极慢,列入看板待议区)。

## 4. hook-bridge 事件协议(Claude Code 增强档)

- 事件文件:`%LOCALAPPDATA%\AgentTrackerIsland\events\claude-code.jsonl`(append-only;2026-09-17 项目更名后路径,旧 AgentTracker 目录数据留在原地未迁移)
- 安装:设置页"启用精确状态"按钮→向 `~\.claude\settings.json` 的 hooks 注入
  SessionStart/UserPromptSubmit/PreToolUse/PostToolUse/Stop/Notification/SessionEnd
  各一条:`node "<home>\.claude\hooks\hook-bridge.js"`(async,注入 timeout 10s,
  桥脚本自带 2s 兜底退出)
  注入采用**读取-合并-写回**且先备份 settings.json.bak;卸载=精确移除自己注入的条目
- hook-bridge.js:stdin 读 JSON → append `{"ts":...,"hook":...,"session_id":...,
  "tool_name":?,"message":?}` → 退出
  (不连端口不找进程;主程序死活无关——红线②)
  > 📌 T6 实测回写(2026-09-16):事件名不带命令行参数,从 stdin 的
  > `hook_event_name` 读取;stdin 无 model 字段,协议已去 model(模型信息由 T4
  > 转录解析补齐);桥脚本落盘位置为 `~\.claude\hooks\`(单一已知位置,用户可审计)
- ⚠️ T6 开工第一步:先装诊断 hook 打印真实 stdin 字段(文档站不可达,以实测为准)

## 5. 灵动岛窗口规格(Tauri)

| 项         | 配置                                                                                         |
| --------- | ------------------------------------------------------------------------------------------ |
| 主窗 island | decorations:false, always_on_top, skip_taskbar, transparent, resizable:false, shadow:false |
| 位置        | 默认顶部居中(计算 workArea);拖拽后坐标存 app_settings                                                    |
| 效果        | 窗口全透明 + CSS 自绘背景(Acrylic 已弃用,见 §1 回写)                                   |
| 收缩态       | 高~40px 胶囊:状态灯(8px 圆点)+ "AgentTrackerIsland"或聚合徽标 + GLM 额度%                                       |
| 展开态       | hover 展开(max-height 过渡):会话卡片区(每卡:状态点/Agent图标/模型/项目名/本会话 token)+ 额度区(5h/周双条+倒计时)            |
| 展开 focus  | non-activating:展开不调 set_focus,pointerLeave 收起(红线⑤)                                         |
| 托盘        | 右键菜单:显示/隐藏灵动岛、设置…、退出(左键同弹菜单)                                                                    |
| 状态灯       | working=呼吸绿(CSS animation);idle/done=常亮绿;waiting=琥珀;error=红(微脉冲);offline=灰                 |

> 📌 M0 落地回写(2026-09-17):托盘落地为三菜单项(显示/隐藏灵动岛、设置…、退出),
> "暂停监控"未纳入(冻结需求未要求,所有者拍板不做);左键同样弹出菜单。
> M1-1 追加"报表…"入口,现共四菜单项。
>
> 📌 M1 追加落地(2026-09-17,所有者追加需求):岛支持自由拖拽 + 贴边自动隐藏——
> 拖放到屏幕上/左/右边缘 24px(逻辑)内自动吸附,吸附后滑出屏外仅留独立信息标签
> (顶部=胶囊底部 1/5 短条 14px,左右=半圆 D 形伸出 20px),鼠标移入滑入显示;
> 设置项 island_autohide 默认开启。交互细节:拖拽防抖 180ms + 左键检测,滑动动效
> 约 200ms,吸附优先级 上>左>右(实现见 lib.rs IslandMotion,几何纯函数有单测)。
> **岛宽自适应**:显示器逻辑宽 × 30%,夹取 [380, 800](island_width;比例参考三分律
> 1/3 与黄金分割小段 0.382 之间的主流悬浮组件区间,所有者笔记本 +1/5 手感校准);
> Rust 贴边几何与前端渲染经 island_metrics command 共用同一结果。全链路使用逻辑坐标系。
> **悬停/点击展开**:设置项 hover_expand(默认开)——开启时悬停岛即展开信息卡片;
> 关闭时需点击岛展开、再点收回,移出岛仍自动收起;设置变更经 hover-expand-changed
> 事件实时推送岛窗口。
> **Agent 身份色与选择(设置项 agents_enabled / agent_colors)**:颜色 = Agent 身份
> (ZC 绿 / CC 橙 / Codex 蓝 / Claude Desktop 紫,设置页可自定义,重复颜色保存拦截);
> 隐藏态等宽分段与面板徽标共用同一身份色;状态用亮度/动效表达(工作中 4s 慢呼吸/
> 等待 1.2s 快闪/出错红圈+快闪/空闲 45% 暗淡/离线近隐没)。勾选才采集/监控/展示
> (Aggregator 跳过未勾选 Agent 的扫描与采集,设置键缺省=全启用);额度耗尽不改写
> 会话状态,由快照 quota_exhausted 驱动胶囊变红/弧线红/红光边框。

## 6. 设置页(内嵌 WebView 路由 #settings)

- GLM 平台(bigmodel/z.ai)+ key(密码框不回显;留空保存=沿用已存/自动发现链,不覆盖);
- 额度提醒阈值(80/95,琥珀须小于红色);统计数据保留周期(8 档);
- Claude Code hooks 安装/卸载;开机自启(默认关);
- 外观(M1-4):主题三选一 system(默认)/dark/light,存 app_settings `theme` 键,
  保存后 emit theme-changed 广播,岛/设置/报表三窗口即时切换;
  跟随系统经 WebView2 PreferredColorScheme→matchMedia 感知,零 Rust 参与;
- 灵动岛(M1-6):贴边自动隐藏开关、悬停/点击展开开关、监控 Agent 勾选与身份色自定义;
  settings/report 窗口原生标题栏颜色跟随系统,不随主题选择(内容区跟随)。

## 7. 风险与对策(实施期)

| 风险                     | 对策                              |
| ---------------------- | ------------------------------- |
| ZCode Schema 随版本漂移     | 列白名单+未知列忽略;scan 失败静默降级为"仅进程监控"  |
| Claude Code JSONL 格式变化 | 同上;对账任务(A2)作为回归项                |
| GLM API 变更             | 集中 glm.rs;快照降级展示                |
| Acrylic 在部分驱动下闪烁       | 设置项可关毛玻璃,退纯色                    |
| settings.json 与其他工具竞争写 | 写前备份+原子写(temp+rename);冲突时提示用户手查 |
| hooks stdin 字段与预期不符    | T6 诊断 hook 实测先行                 |
