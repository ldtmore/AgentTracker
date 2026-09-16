# AgentTrackerIsland 项目计划(v1,M0 详细设计)

> 状态:**草案已归档**——2026-09-16 起改用三阶段流程(需求梳理 → 调研回填 → 实施),
> 见 [WORKFLOW.md](WORKFLOW.md)。本文件保留作为阶段 2 的输入材料(其中的技术判断仍可参考),
> 正式计划将在阶段 2 结束时产出 `02-DESIGN.md` + `03-TASKS.md` 取代本文件。
> 编写日期:2026-09-16 | 上游调研:E:\AIAgentTemp\AgentTracker-research\

## 0. 环境现状与前置决策(需要你拍板)

| 项 | 现状 | 影响 |
|---|---|---|
| Node.js | ✅ v24.20.0 + npm 11.19.0 | 前端工具链就绪 |
| Rust 工具链 | ❌ 未安装(rustc/cargo 均无) | **Tauri 2 方案的前置条件,必须先装** |
| VS Build Tools(MSVC) | 未确认 | Rust on Windows 编译需要;若无则一并安装 |

### ⚠️ 决策点 1:是否安装 Rust 工具链(约 1~3 GB,一次性成本)

| 选项 | 结论 |
|---|---|
| **A. 安装 Rust + MSVC Build Tools(推荐)** | `winget install Rustlang.Rustup` + VS Build Tools。Tauri 方案成立:成品内存 50~80MB、安装包 <10MB,真正"轻量";cc-switch(133k⭐)与 CodeZeno(487⭐)均为 Rust 系,已验证此路 |
| B. 改用 Electron | 免装 Rust,但内存 200MB+、安装包 80MB+,与"轻量"定位直接冲突,不推荐 |
| C. 改用 C# WPF | 免装 Rust(需 .NET SDK),Win 原生最轻,但 UI 生态弱、后续跨平台(macOS/Linux)基本断头 |

> 推荐 A。若你同意,安装动作会在计划批准后、编码前执行。

---

## 1. 技术选型(定稿)

| 层 | 选型 | 版本基线 | 说明 |
|---|---|---|---|
| 应用框架 | Tauri 2 | 2.x stable | 轻量桌面壳,Rust 后端 + 系统 WebView |
| 后端语言 | Rust | stable(2024 edition) | 采集器/状态机/适配器全部在 Rust 侧 |
| 前端 | React 18 + TypeScript + Vite | - | 生态最大,ECharts 集成成熟 |
| 存储 | rusqlite(bundled)+ WAL | - | SQLite 单文件,零运维 |
| 图表 | Apache ECharts | 6.x | M1 报表页用,M0 预留 |
| 文件监听 | notify crate | 6.x | 监听转录文件变化 |
| 配置 |tauri-plugin-store / 单 json 文件 | - | 简单KeyValue即可,YAGNI |

## 2. 总体架构

```
┌─────────────────────────── 展示层(WebView) ───────────────────────────┐
│  IslandWindow 灵动岛(常驻置顶)      ReportWindow 报表页(M1)          │
└──────────────────────────────┬────────────────────────────────────────┘
                               │ Tauri command / event
┌──────────────────────────────┴────────────────────────────────────────┐
│                      核心服务(Rust,单进程内后台线程)                  │
│  ┌────────────┐  ┌──────────────┐  ┌────────────────────────────┐     │
│  │ 状态聚合器  │←─│ 采集器组      │  │ ProviderAdapter(额度)      │     │
│  │ 状态机+看门狗│  │ hooks事件消费 │  │  ├ glm.rs  Monitor API     │     │
│  └────────────┘  │ jsonl转录监听 │  │  └ anthropic.rs(M1,OAuth)  │     │
│        ↑         │ 进程枚举兜底  │  └────────────────────────────┘     │
│        │         └──────────────┘                                      │
│  ┌────────────┐                                                        │
│  │ SQLite 存储 │  sessions / usage_records / quota_snapshots / events  │
│  └────────────┘                                                        │
└───────────────────────────────────────────────────────────────────────┘
```

## 3. M0 范围(明确边界)

**做**:Claude Code(含 GLM 供应商场景)的 ①灵动岛三态监控 ②token 用量统计 ③GLM 5h/周额度+重置倒计时 ④会话悬浮详情(点击跳转窗口)。
**不做**(推 M1+):Codex/其他 Agent、Anthropic 官方额度、报表页、悬浮球模式切换、声音通知、多语言。

## 4. 详细设计

### 4.1 目录结构

```
F:\MyProjectRepository\AgentTrackerIsland\
├─ AGENTS.md                  # 设计宪法
├─ docs\PLAN.md               # 本计划
├─ src\                       # 前端(React + TS)
│  ├─ island\                 #   灵动岛组件(收缩态/展开态/状态灯)
│  ├─ report\                 #   报表页(M1 占位)
│  └─ shared\                 #   类型定义(与 Rust 侧 serde 共享结构)
└─ src-tauri\                 # Rust 后端
   ├─ src\
   │  ├─ main.rs              # 入口,窗口创建
   │  ├─ collector\
   │  │  ├─ mod.rs            #   采集器调度(线程池+水位)
   │  │  ├─ hook_events.rs    #   L2:hooks 事件文件消费
   │  │  ├─ transcript.rs     #   L1:JSONL 转录增量解析(含回溯补录)
   │  │  └─ process_probe.rs  #   L0:进程/窗口枚举兜底
   │  ├─ provider\
   │  │  ├─ mod.rs            #   ProviderAdapter trait
   │  │  └─ glm.rs            #   GLM Monitor API + 积分换算
   │  ├─ state\
   │  │  └─ mod.rs            #   状态机、三级融合、看门狗
   │  ├─ store\
   │  │  ├─ mod.rs            #   SQLite 封装
   │  │  └─ migrations\       #   Schema 迁移脚本
   │  └─ commands.rs          #   Tauri command(前端调用入口)
   └─ hook-bridge\            # 发给 Claude Code 的 hook 桥(独立小脚本)
      └─ hook-bridge.js       #   单文件,append 事件到固定文件即退出
```

### 4.2 数据库 Schema(SQLite)

```sql
-- 会话表:一个 Agent 会话一行
CREATE TABLE sessions (
  id            TEXT PRIMARY KEY,      -- 如 claude-code:{sessionId}
  agent         TEXT NOT NULL,         -- 'claude-code' | 'codex' | ...
  project_dir   TEXT,                  -- 工作目录(从转录文件路径推得)
  provider      TEXT,                  -- 'glm' | 'anthropic' | ... (由 session-env/base_url 判定)
  first_seen_at INTEGER, last_seen_at INTEGER,
  state         TEXT DEFAULT 'offline' -- online/working/idle/waiting/error
);

-- token 流水:每条 assistant 消息一行(增量,不重复入库)
CREATE TABLE usage_records (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id  TEXT NOT NULL,
  model       TEXT NOT NULL,           -- 'glm-5.3' / 'glm-5.3-flash'
  ts          INTEGER NOT NULL,        -- 消息时间戳
  input_tokens INTEGER, output_tokens INTEGER,
  cache_read_tokens INTEGER, cache_creation_tokens INTEGER,
  UNIQUE(session_id, ts, model)        -- 幂等键:回放补录不重复
);
CREATE INDEX idx_usage_ts ON usage_records(ts);

-- 额度快照:定时拉取,保留历史(M1 报表画额度曲线用)
CREATE TABLE quota_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider TEXT NOT NULL,              -- 'glm'
  window_kind TEXT NOT NULL,           -- '5h' | 'weekly'
  used_percent REAL, used_tokens INTEGER,
  reset_at INTEGER,                    -- 重置时间戳
  fetched_at INTEGER NOT NULL
);

-- hooks 状态事件(hook-bridge 写入的原始事件,审计+回溯)
CREATE TABLE status_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT, agent TEXT, hook TEXT,  -- PreToolUse/Stop/Notification/...
  payload JSON, ts INTEGER NOT NULL
);
```

### 4.3 状态机与三级数据源融合

状态:`online → working ⇄ idle → waiting → ... → offline`,外加 `error`。

| 优先级 | 数据源 | 提供的信息 | 缺失时 |
|---|---|---|---|
| L2 hooks 事件 | hook-bridge → 事件文件 | 精确状态迁移(见 4.4 映射) | 降 L1 |
| L1 转录监听 | notify 监听 `~\.claude\projects\**\*.jsonl` | token 流水 + "文件 90s 内有写入 ⇒ working" 近似 | 降 L0 |
| L0 进程枚举 | 枚举 claude 进程/终端窗口 | 仅"Agent 在线" + 点击跳转目标 | 显示"未检测到" |

**看门狗**(借鉴 Wu-Hao-chen 面板设计):working 超过 5 分钟无任何新事件/文件写入 → 自动回 idle,防止状态卡死;error 态不受看门狗影响。

**error 判定**:Notification hook 消息含 rate_limit/额度字样、或额度快照 =100%、或转录出现 error 记录 → error,并在详情里写明原因(额度耗尽/进程退出/API 报错)。

### 4.4 hooks 事件文件协议(故障隔离的核心)

**原则:hook 桥只写文件,永不停留。** 主程序不在时它照样秒成功,Agent 零感知。

- 事件文件:`%LOCALAPPDATA%\AgentTrackerIsland\events\claude-code.jsonl`(append-only)
- 注入 `settings.json` 的 hook 命令(首启向导自动写入,设置页一键卸载):
  `node "<安装目录>\hook-bridge\hook-bridge.js" %CLAUDE_HOOK_NAME%`
  (hook-bridge 从 stdin 读 Claude Code 传入的 JSON,取 session_id/model 等,append 一行事件,退出)
- 事件行格式:`{"ts":...,"hook":"Stop","session_id":"...","model":"glm-5.3"}`

**hook → 状态映射**:

| Claude Code hook | AgentTrackerIsland 状态 |
|---|---|
| SessionStart | online |
| UserPromptSubmit / PreToolUse / PostToolUse | working |
| Stop | idle(回合完成) |
| Notification | waiting;消息含额度/限流关键词 ⇒ error |
| SessionEnd | offline |

### 4.5 GLM ProviderAdapter

```
trait ProviderAdapter {
  fn id(&self) -> &str;                        // "glm"
  fn fetch_quota(&self) -> Result<QuotaSnapshot>;  // 5h + weekly + reset_at
}
```

- 请求:`GET {base}/api/monitor/usage/quota/limit`,Header `Authorization: <key>`(无 Bearer;base = open.bigmodel.cn 或 api.z.ai,设置页选择;key 可默认取环境变量 ANTHROPIC_AUTH_TOKEN)
- 刷新策略:5 分钟定时 + 手动;超时 5s;失败降级显示"最近一次快照(时间)"
- **积分换算**(UI 双口径:原始 token / 套餐积分):
  - glm-5.3 / glm-5-turbo:工作日 14:00–18:00(UTC+8)×3 系数,其余 ×1
  - glm-4.7:全天 ×1
  - 口径来自官方文档 docs.bigmodel.cn/cn/coding-plan;换算规则做成配置表,官方调整时只改配置

### 4.6 灵动岛窗口交互规格(M0)

| 项 | 规格 |
|---|---|
| 位置 | 屏幕顶部居中,贴顶停靠 |
| 收缩态 | 胶囊形,高度 ~36px:状态灯 + Agent 名/模型 + 额度百分比 |
| 状态灯 | 🟢呼吸绿=working(M0 用 CSS 动画呼吸而非快闪,减少视觉骚扰)、🟢常亮=idle/完成、🟡=waiting、🔴=error、灰=offline |
| 展开态 | hover 展开(向下生长,non-activating 不抢焦点):会话卡片列表(状态/模型/项目名/本会话 token)+ GLM 5h/周额度条与重置倒计时 |
| 点击行为 | 点会话卡片 → 激活对应终端窗口(进程窗口枚举 + SetForegroundWindow) |
| 其他 | 可拖拽换位;右键菜单:设置/报表(M1)/退出;无任务栏图标,托盘常驻 |

## 5. 任务拆解(每步可独立验证)

| # | 任务 | 产出/验收 | 依赖 |
|---|------|----------|------|
| T0 | 安装 Rust 工具链 + VS Build Tools | `cargo --version` 可用 | 决策点 1 |
| T1 | Tauri 2 脚手架(react-ts 模板)+ 编译跑通空窗口 | `npm run tauri dev` 出窗口 | T0 |
| T2 | SQLite 存储层 + migrations(4.2 全部表) | 单元测试:建表/幂等写入/回放不重复 | T1 |
| T3 | transcript 采集器:解析 `~\.claude\projects\**\*.jsonl` → usage_records(含全量回溯 + notify 增量) | 对本机真实数据跑:最近 7 天 glm-5.3 统计数与 ccusage 口径一致 | T2 |
| T4 | GLM 适配器:fetch_quota + 定时刷新 + 降级 | 用真实 key 拉 5h/周百分比与官网一致 | T2 |
| T5 | hooks 链路:hook-bridge.js + 事件文件消费 + 向导注入/卸载 | 手动触发 hook,事件入库;卸载后 settings.json 还原 | T2 |
| T6 | 状态聚合器:三级融合 + 看门狗 + error 判定 | 模拟测试:拔掉任一层,状态仍可用 | T3,T5 |
| T7 | 灵动岛 UI:收缩/展开/状态灯/会话卡/额度条 | 对照 4.6 规格逐项过 | T4,T6 |
| T8 | 点击跳转终端窗口 | 从岛内激活 Windows Terminal 会话 | T7 |
| T9 | 打包安装包(NSIS)+ 冒烟 | 干净机器装/卸载,无残留、Agent 无感知 | T7,T8 |

**关键里程碑**:T3 完成即"数据正确性"验证点(用你本机真实数据对标 ccusage);T7 完成即可日常自用(Dogfood)。

## 6. 风险与对策(编码前已知)

| 风险 | 概率 | 对策 |
|---|---|---|
| GLM Monitor API 变更/不稳定 | 中 | 只读+快照缓存+降级提示;接口路径集中在 glm.rs 一处 |
| JSONL 格式随 Claude Code 版本漂移 | 中 | 解析器容忍未知字段;T3 用真实数据回归;失败行记录日志不中断 |
| hooks 注入被用户其他工具(cc-switch 残留/自写脚本)覆盖 | 中 | 设置页可检测 hook 是否在位并提示重装;L1/L0 兜底保证可用 |
| Windows 焦点抢占(展开面板抢焦点违背红线 5) | 低 | Tauri 前台失焦收起 + accept_first_mouse 配置验证 |
| Rust 学习曲线(若你不熟 Rust) | - | M0 代码量集中在 4 个模块,结构已在 4.1 定死;每模块可由 AI 辅助生成后人工审 |

## 7. M1/M2 展望(仅备忘,不承诺)

- **M1**:报表页(按供应商/模型/时间聚合、趋势图、热力图)、Codex CLI、Anthropic 官方额度(OAuth)、托盘快捷菜单
- **M2**:悬浮球模式、Trae/更多 Agent、错误诊断详情、开源发布(定位叙事:「Windows 端 Vibe Island 开源替代,但只观测不经手流量」)

---

## 附:待你确认的决策清单

1. ⚠️ **决策点 1**:技术栈选 A(Tauri+Rust,需安装 Rust 工具链 1~3GB)/ B(Electron)/ C(WPF)?
2. 本计划整体(M0 范围、状态语义、hooks 事件文件方案)是否批准?有要增删的部分吗?
3. 批准后我按 T0→T9 顺序执行,每完成一个里程碑(T3/T7)向你汇报一次。
