# AgentTracker 任务看板(03-TASKS)

> 阶段 3 执行清单 | 创建:2026-09-16 | 配套:[02-DESIGN](02-DESIGN.md)
> 规则(见 [WORKFLOW](WORKFLOW.md)):每任务完成即更新本看板状态与 [HANDOFF](HANDOFF.md);
> 做不完的任务如实记录进度后再结束会话;禁止看板外扩 scope

## 状态图例

⬜ 待办 | 🟨 进行中 | ✅ 完成(附日期/验收证据) | ⛔ 阻塞(附原因)

## M0 任务序列

### T0 环境准备 ✅(2026-09-16 完成)

- 内容:安装 Rust(rustup,stable-msvc,`--profile minimal`)+ VS Build Tools(MSVC C++ 工作负载);
  目录按所有者 D 盘布局:`D:\Rust\{rustup,cargo}`(RUSTUP_HOME/CARGO_HOME)+ `D:\VSBuildTools`
- 验收:✅ rustc 1.98.1 + cargo 1.98.1(D:/Rust,PATH 已含 D:\Rust\cargo\bin);
  ✅ cl.exe 在 D:\VSBuildTools\VC\Tools\MSVC\14.44.35207(vs_BuildTools.exe 直装,winget 本机不可用)
- 备注:安装细节与坑见 [HANDOFF](HANDOFF.md)

### T1 项目骨架 ✅(2026-09-16 完成)

- 内容:`F:\AgentTracker` 下初始化 Tauri 2 + React/TS 模板,按 02-DESIGN §1 建目录
  (src-tauri/src/{collector,provider,state,store} + src/{island,shared});补充 .gitignore、git init
- 验收:✅ `tauri dev` 打开窗口(PID 2880 实测,内存 ~42MB);✅ `cargo build` 全量通过(6m23s)
  + 前端 `npm run build` 通过;debug 产物 target/debug/agenttracker.exe 生成
- 备注:模块子目录(collector/provider 等)尚未创建——T2 起按需建立;
  模块子目录随对应任务落地(避免空目录)
- 依赖:T0 ✅

### T2 存储层 ✅(2026-09-16 完成)

- 内容:rusqlite(bundled)+ migrations/0001_init.sql(02-DESIGN §3 全表)+ 增删查封装 + 水位管理
- 验收:✅ 单测 4/4 通过(迁移幂等/用量去重/水位单调/滚动清理,0.06s);
  依赖 rusqlite 0.40.2 + anyhow(错误统一,后续适配器复用)
- 依赖:T1 ✅

### T3 ZCode 适配器 ✅(2026-09-16 完成)

- 内容:zcode.rs——只读打开 `~\.zcode\cli\db\db.sqlite`,列白名单读取 model_usage,
  水位增量采集入库,会话元数据 join session 表,状态启发式数据(last_usage_at)
- 验收:✅ 集成测试 2/2(连本机真实库):全量采集 582 行入库幂等、水位增量正确、
  字段断言通过(毫秒时间戳/模型/agent);采集全程 ZCode 运行中(本会话即 ZCode)零影响
- 备注:provider 由模型名前缀启发式映射(ZCode 的 provider_id 是内部 UUID);
  历史数据存在大小写混用(GLM-5.3/glm-5.3),比较一律 to_ascii_lowercase
- 依赖:T2 ✅

### T4 Claude Code 适配器 ✅(2026-09-16 完成)

- 内容:claude_code.rs——扫描 `~\.claude\projects\**\*.jsonl`,解析 assistant usage,
  messageId(+requestId)去重保留最大快照,文件 mtime 过滤 + 时间水位增量,synthetic 行过滤
- 验收:✅ 单测 5/5 + 集成 4/4 全绿;
  ✅ **A2 对账完成**(详见 [01-RESEARCH §8](01-RESEARCH.md)):516 条消息/27.98M tokens,
  与 ccusage 差 1.7%,差异来源已书面定位(ccusage 额外纳入 cost-state 非助手行源;
  本机数据无 requestId,去重口径与 ccusage/better-ccusage 完全同款)
- 里程碑:🎯 数据正确性关口通过,可继续 T5+
- 依赖:T2 ✅

### T5 GLM Provider ✅(2026-09-16 完成)

- 内容:provider/{mod,glm}.rs——ProviderAdapter trait、Monitor API 调用(5s 超时)、
  实测响应解析(TOKENS_LIMIT 区分 5h/weekly、percentage=已用%、重置毫秒时间戳)、
  凭据发现链(设置→环境变量→~\.claude\suppliers.json 自动发现)
- 验收:✅ 单测 7/7 + 真实调用通过:**凭据自动从 claude-menu 配置发现**;
  实测 [5h] 已用 100%(本会话消耗中)/ [weekly] 54%,重置时间正确解析;
  API 即官网数据源,百分比与官网必然一致(所有者可随时官网复核)
- 备注:① 定时刷新/失败降级调度在 T7 聚合器统一实现(后台线程所在层);
  ② ⚠️ chrono 显示的是 UTC——UI 层倒计时/时间须转 Asia/Shanghai 本地时区
- 依赖:T2 ✅

### T6 hooks 链路 ✅(2026-09-16 完成)

- 内容:①诊断 hook 实测 stdin 字段(公共:session_id/transcript_path/cwd/hook_event_name;
  专有:tool_name/message/prompt_id 等);②hook-bridge.js(白名单提取→事件文件 append,
  2s 兜底退出);③Rust 事件消费者(字节偏移增量读,半行安全);④install/uninstall_hooks
  (桥脚本编译进二进制,settings.json 备份+合并注入+防重复+原子写)
- 验收:✅ 单测 9/9;✅ 端到端实测:安装→`claude -p` 触发→**捕获 3 个真实事件**
  (SessionStart/UserPromptSubmit/SessionEnd)→消费→卸载后 settings 与原始状态语义等价;
  Drop 守卫保证测试 panic 不留注入残留
- 备注:①stdin 无 model 字段,02-DESIGN §4 事件协议已去 model(模型信息由 T4 补);
  ②Windows 下 Rust 调 npm shim 需 `cmd /c`(不走 PATHEXT);③事件协议含 message 字段
  (Notification 的限流关键词判定,T7 用);④"10s 内反映到岛状态"在 T7/T9 联动验收
- 依赖:T2 ✅

### T7 状态聚合器 ✅(2026-09-16 完成)

- 内容:state/{mod,service}.rs——状态机纯函数(error 判定:限流正则/ZCode error_type/额度 100%;
  hooks 事件驱动;看门狗 5min;启发式 90s 活跃窗;进程枚举兜底 sysinfo)+ Aggregator 服务
  (10s tick:双适配器增量采集→入库→融合→快照;GLM 5min 刷新+失败降级最近快照;
  5h 额度 100% 时活跃会话标红)
- 验收:✅ 单测 13/13(状态全矩阵/看门狗/聚合优先级);✅ e2e 本机真实数据:
  46 会话融合,岛 AnyError(GLM 5h 100% 正确触发),ZCode 本会话 47M tokens 统计正确;
  **当前 hooks 未安装(增强档已卸),状态来自启发式层——"拔掉 hooks 仍可用"已实测证明**
- 备注:设计缺陷被单测逮住并修正——hooks 事件有效窗口须大于看门狗窗口(30min vs 5min),
  否则看门狗分支不可达;quota 100% 的会话标红属产品口径,后续可在设置里提供开关
- 依赖:T3,T4,T5,T6 ✅

### T8 岛 UI-壳 ✅(2026-09-16 完成)

- 内容:tauri.conf.json 岛窗口(decorations:false/alwaysOnTop/skipTaskbar/transparent/
  resizable:false/shadow:false)+ 顶部居中定位与拖拽坐标记忆(Moved→app_settings)+
  托盘(显示/隐藏/退出)+ 后台聚合线程(10s tick→emit island-snapshot)+ 前端收缩态
  (胶囊/状态灯呼吸脉冲/摘要文案/token 缩写,CSS 自绘背景)
- 验收:✅ 形态经所有者屏幕实测确认;进程内存 39–48MB(含聚合器,红线 ≤100MB);
  数据库落盘 %APPDATA%\com.agenttracker.app\agenttracker.db,10s 刷新闭环
- 备注:① Acrylic 弃用——window-vibrancy 是**窗口级**效果,把整个矩形染灰破坏胶囊
  形态;正确做法=窗口全透明+CSS 自绘(依赖保留,M1 全宽形态可再评估);
  ② 拖拽位置记忆与托盘菜单的交互行为并入 T12 冒烟一并验收
- 依赖:T7 ✅

### T9 岛 UI-内容 ✅(2026-09-16 完成)

- 内容:src/{shared/types.ts,island/{IslandBar,Panel}.tsx,App.tsx,App.css}——
  hover 展开(窗口高度 48→520 动态调整,宽恒定避免锚点跳变,不抢焦点);
  会话卡片区(活跃优先排序/徽标 CC·ZC/模型/项目/token/状态,≤30 条滚动);
  GLM 额度区(5h/周双进度条+百分比+倒计时本地 30s 推进);
  静默提醒变色(≥80% 琥珀/≥95% 红,收缩态与进度条同步变色)
- 验收:✅ 所有者屏幕实测确认(展开/收起/卡片/额度条/变色);
  数据 10s 刷新闭环(T8 已验);capabilities 增 set-size/set-position 权限
- 依赖:T7,T8 ✅

### T10 点击跳转 ⬜

- 内容:点击会话卡 → 定位该会话所属终端/IDE 窗口(windows-rs 枚举+前台进程关联)→ SetForegroundWindow
- 验收:**A4**——双终端各跑 Claude Code/ZCode 场景下,点卡片分别激活正确窗口
- 依赖:T9

### T11 设置页 ⬜

- 内容:GLM 平台/key、提醒阈值、数据清理周期(8 档)、hooks 开关、开机自启(默认关)、毛玻璃开关
- 验收:设置持久化重启生效;key 不出现在日志
- 依赖:T5,T6,T8

### T12 验收与打包 ⬜

- 内容:红线回归(A6:退出/卸载工具后两 Agent 无报错)+ 性能(A5:内存≤100MB、冷启动≤2s)+
  便携 zip( portable 产物+首次运行说明)
- 验收:A5/A6 通过;zip 解压即用;附验收记录写入本文档
- 依赖:T3–T11

### T13 Dogfood 周 ⬜

- 内容:所有者日常自用 ≥1 周,问题记录到看板"待议区"
- 验收:**A7**——期间不再手动查官网额度;汇总问题清单定 M1 输入
- 依赖:T12

## 待议区(看板外想法,不擅自实施)

- (空)

## 交接锚点

- 任何 Agent 接手:先读 [HANDOFF.md](HANDOFF.md)(当前进度/坑/下一步),再从最靠前的 ⬜/🟨 任务继续
- M0 验收标准全文见 [00-REQUIREMENTS](00-REQUIREMENTS.md) 维度⑩
