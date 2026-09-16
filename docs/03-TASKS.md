# AgentTrackerIsland 任务看板(03-TASKS)

> 阶段 3 执行清单 | 创建:2026-09-16 | 配套:[02-DESIGN](02-DESIGN.md)
> 规则(见 [WORKFLOW](WORKFLOW.md)):每任务完成即更新本看板状态与 [HANDOFF](HANDOFF.md);
> 做不完的任务如实记录进度后再结束会话;禁止看板外扩 scope;
> **构建打包仅在所有者明确宣布正式对外发布时执行**(见 WORKFLOW 构建打包纪律)

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

- 内容:`F:\MyProjectRepository\AgentTrackerIsland`(更名前为 F:\AgentTracker,见 HANDOFF)下初始化 Tauri 2 + React/TS 模板,按 02-DESIGN §1 建目录
  (src-tauri/src/{collector,provider,state,store} + src/{island,shared});补充 .gitignore、git init
- 验收:✅ `tauri dev` 打开窗口(PID 2880 实测,内存 ~42MB);✅ `cargo build` 全量通过(6m23s)
  + 前端 `npm run build` 通过;debug 产物 target/debug/agenttrackerisland.exe 生成
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
  数据库落盘 %APPDATA%\com.agenttrackerisland.app\agenttrackerisland.db(2026-09-17 更名后路径),10s 刷新闭环
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

### T10 点击跳转 🟨(2026-09-16 部分完成,所有者决定不阻塞)

- 内容:commands.rs——窗口枚举(EnumWindows+PID)+ 四级匹配(标题含完整路径 >
  目录名 > 进程链匹配(跑 claude 的 pwsh→父链→WT 宿主窗口)> Agent 关键词)+
  focus_session command;前端卡片 onClick
- 验收:✅ **A4 部分通过**:ZCode 卡片 → ZCode 桌面窗口激活正常;
  ❌ Windows Terminal(PowerShell 7 + Claude CLI)未命中——所有者指示搁置,
  不阻塞 M0(详见待议区)
- 依赖:T9 ✅

### T11 设置页 ✅(2026-09-16 完成)

- 内容:settings 窗口(#settings hash 分流)+ Settings.tsx 五区块(GLM 平台/key 密码框、
  提醒阈值、清理周期 8 档、hooks 启用/停用、开机自启 tauri-plugin-autostart);
  Rust commands(get_settings/set_setting/hooks_*/autostart_*);GLM 凭据优先级改
  应用设置>env>claude-menu;启动时按周期执行数据清理;设置窗口关闭即隐藏(可反复唤起)
- 验收:✅ 所有者实测通过(五区块正常/设置窗口反复唤起修复);
  key 仅存本地 app_settings 不入日志;凭据/阈值重启生效已明示
- 依赖:T8 ✅(hooks 命令复用 T6)

### T12 验收与打包 ⏸(构建打包=正式发布动作:仅当所有者明确宣布对外发布时执行,见 WORKFLOW 构建打包纪律)

- 内容:红线回归(A6:退出/卸载工具后两 Agent 无报错)+ 性能(A5:内存≤100MB、冷启动≤2s)+
  便携 zip(release 产物+首次运行说明)
- ✅ 前置已完成:bundle active=false;代码细节调整 12 项(2026-09-16,提交 b0a1652);
  UI 改动所有者已屏幕验收(累计前缀/阈值变色/保存自动关闭/滚动条)
- 📌 拍板回写(2026-09-17):托盘"暂停监控"、额度手动刷新、error 原因展示三项均不做,
  已回写 02-DESIGN 相应小节
- 待办(dev 即可验,所有者):①A1 waiting 场景——设置页启用 hooks,让 CC 等待输入,
  岛应变琥珀;②拖拽岛→重启→位置还原;③托盘三菜单项各验一遍
- 待办(随正式发布执行):release 构建 + A5 性能实测 + A6 红线回归 + 便携 zip
  (触发条件=所有者明确宣布打包发布,平时一律不执行)
- 依赖:T3–T11(T10 部分完成已获所有者接受)

### T13 Dogfood 周 ⬜

- 内容:所有者日常自用 ≥1 周,问题记录到看板"待议区"
- 验收:**A7**——期间不再手动查官网额度;汇总问题清单定 M1 输入
- 依赖:T12

## M1 任务序列(2026-09-17 启动,优先级已与所有者确认)

> 顺序:P1→P6 逐项开发;构建打包不在此列(WORKFLOW 构建打包纪律);
> 每项开工前先调研(不造轮子:🟢直接依赖/🟡借鉴实现/🔴自研)

### M1-1 报表页 🟨(P1,代码完成+自验通过,待所有者过目)

- 内容:独立窗口(#report 路由,与设置页同模式);趋势图(按日 token)、热力图(周×小时
  用量分布)、按模型/供应商聚合占比、时间范围切换(7/30/90 天/全部);
  数据源=自库 usage_records;入口=托盘菜单"报表…";ECharts 按需引入
- ✅ 代码完成(2026-09-17):store 四个聚合查询(report_daily/by_model/by_provider/heatmap,
  日界/星期/小时用 SQLite 'localtime' 取本机时区)+ 单测 test_report_aggregates;
  托盘"报表…"入口 + 关窗即隐藏(与设置页同模式);前端 lazy 分割——echarts 独立
  chunk 仅报表窗口加载,岛主包只 +2KB;调研结论见 01-RESEARCH §9
- ✅ AgentTrackerIsland 实例内屏幕自验(2026-09-17):四图+范围切换+滚动全正常,真实数据
  (09-16/09-17 单日 ~190M);发现并修复模型名大小写切片问题(report_slice 按小写归一)
- 验收:所有者过目确认
- 依赖:无

### M1-2 搁置问题统一优化 ⬜(P2)

- 内容:所有者暂记的待优化清单(**待所有者补录**到本看板待议区),逐项修复
- 依赖:所有者提供清单

### M1-3 Codex CLI 接入 🟨(P3,源码级调研完成,适配器待实测条件)

- 内容:第一步先调研其本地数据格式(转录/事件/token 记录),勘察结论回填 01-RESEARCH;
  适配器实现待所有者实际使用 Codex 后进行(无真实数据不可验证)
- ✅ 调研完成(2026-09-17,源码级):sessions/*.jsonl(rollout 格式)+ TokenCount 事件
  (input/cached/cache_write/output/reasoning/total),详见 01-RESEARCH §10;
  ⚠️ 新版出现 SQLite 状态库,JSONL 与状态库的读取面取舍待真实样本实测
- 待办:所有者安装并使用 Codex → 取真实 JSONL 样本实测字段 → 实现 AgentAdapter
- 依赖:M1-1;所有者环境

### M1-4 深浅双主题 ⬜(P4)

- 内容:跟随系统+设置页自选;岛/面板/设置页/报表页色板变量化统一
- 依赖:M1-1(避免报表页返工)

### M1-5 悬浮球/任务栏形态 ⬜(P5,视进度)

- 内容:与岛复用同一套数据/组件的形态扩展
- 依赖:M1-1~4 完成后视进度评估

### M1-6 灵动岛贴边自动隐藏 🟨(追加需求 2026-09-17,代码完成待所有者屏幕验收)

- 内容(所有者追加,有望替代悬浮球):岛可自由拖拽到屏幕任意位置;拖放到上/左/右边缘
  24px 内自动吸附;吸附后滑出屏外仅露 6px 边缘,鼠标移入滑入显示;设置页"灵动岛"区块
  控制开关,默认开启
- 实现:lib.rs IslandMotion 状态机(拖拽防抖看护线程:180ms 静默 + GetAsyncKeyState
  左键检测)+ 程序化滑动动画(代数守卫 + animating/programmed 双标记防自触发循环)+
  island_peek/island_refresh/island_drag_start/island_metrics commands + island-dock 前端事件;
  吸附优先级 上>左>右,几何纯函数 detect_edge/hidden_pos 有单测(18/18 绿)
- 迭代记录:①首测暴露拖拽权限缺失(core:window:default 不含 start-dragging,历史遗留);
  ②隐藏几何改固定常量+逻辑坐标系(DPI 缩放适配,200% 屏实测);③贴边隐藏态重设计:
  颜色=Agent 身份(用户可配,重复颜色保存拦截),等宽分段,状态用亮度/动效表达,
  左右为圆心辐射扇形+沿外沿额度弧线(SVG 同曲线描边),顶部含 Agent 标识+底边额度
  发丝线;④岛宽自适应:显示器逻辑宽×30% 夹取 [380,800],island_metrics 同源;
  ⑤额度耗尽不再改写会话状态(quota_exhausted 快照标志),扇形"全红不渲染"根因即此;
  ⑥设置页新增:悬停/点击展开开关、监控 Agent 选择与颜色自定义(勾选才采集);
  ⑦修复拖拽权限缺失、隐藏标签错位竞态、SVG 不渲染等(详见 HANDOFF 踩坑)
- 验收:所有者 dev 实测——自由拖拽 / 三边贴靠滑出 / 标签形状与位置 / 设置开关即时生效 /
  不同分辨率宽度自适应
- 备注:M1-5 悬浮球形态届时再评估是否仍有必要

### M1-7 审查修复与代码↔文档一致性回写 ✅(2026-09-17 完成)

- 触发:全量需求↔代码审查(2026-09-17)发现 3 项功能缺陷 + 多处文档未回写,
  所有者指示"先修复代码问题,再按代码优化文档"
- 修复内容:
  - ① **GLM 凭据回退(P1)**:设置 token 留空时自动回落发现链(env → claude-menu),
    兑现"留空则继续沿用";设置页 Key 不回显、留空保存不覆盖已存 Key;
    凭据来源由聚合器启动时写入 `glm_token_source` 供设置页展示(service.rs/Settings.tsx)
  - ② **用量快照取最大(P2 同键部分)**:幂等键冲突时仅当新行四项合计更大才整行覆盖,
    与 CC 流式去重口径一致;跨 tick 重采到更完整快照可原地升级(store/mod.rs,
    新增单测 test_usage_upsert_keeps_max_snapshot)
  - ③ **进程探测误判(P3)**:hook-bridge 的 node 进程(路径含 .claude,寿命 ≤2s)
    不再被判定为 claude 存活(service.rs)
  - ④ calc_percent 不再拿 currentValue 绝对量冒充百分比,不可换算返回 None(glm.rs)
  - ⑤ 移除状态机 online 死状态(无产出路径;Rust 枚举/前端类型/排序/严重度/CSS 六处同步)
  - ⑥ 修正陈旧注释:App.tsx 岛宽 25%→30%、filterSnap 采集口径、lib.rs saved_pos
    物理→逻辑坐标、state/mod.rs 额度耗尽回写说明
- 文档回写:02-DESIGN §1 技术栈(edition/React/notify-rs/Acrylic/分发)、§2.1 trait
  实际签名+watch 备注、§2.2 凭据链与百分比兜底、§2.3 状态机五态+waiting 聚合+
  quota_exhausted、§3 清理范围、§4 hook 协议实测版、§5 托盘四菜单——与代码一致
- 验证:cargo test **19/19** 通过(新增 1 测)、npm run build 通过、cargo check 零警告
- 依赖:无

### 已划出 M1(2026-09-17 所有者拍板)

- Anthropic 官方订阅额度 → 移回 M2 愿景池(所有者无官方订阅)
- NSIS 安装包 → 归正式发布阶段(WORKFLOW 构建打包纪律)

## 待议区(看板外想法,不擅自实施)

- **CC 跨 tick 流式重复残留**(M1-7 P2 余项):usage_records 幂等键不含 messageId,
  同一消息的多条流式快照行若时间戳不同仍会入库两行(同键取最大快照场景已修复)。
  影响用量准确度,需取真实增量样本测影响量级再定方案(加 message_id 列需迁移 0002)
- **hook 事件文件无轮转**:%LOCALAPPDATA% 事件文件 append-only 无限增长,且每 10s
  tick 全量读取一次;长期运行需补轮转/截断策略
- **CC 采集全量重读**:每 tick 全量重读所有 mtime 新于水位的转录文件;
  scan_sessions 对 90 天内全部转录读 8KB 头后才截断 100——转录量大时改文件内
  偏移增量(即原 02-DESIGN §2.1 的文件内偏移方案)
- **sessions 表不在清理范围**:每会话一行缓慢累积,长期可考虑随数据保留周期清理
- **WT 中 Claude Code 卡片跳转未命中**(T10 遗留):ZCode ✅/标题含路径的场景理论 ✅,
  但所有者环境(Windows Terminal + PowerShell 7)进程链匹配未命中。下次调试线索:
  ①打印 WT 进程树(pwsh 的父链是 WindowsTerminal.exe 还是经 OpenConsole/conhost 中转);
  ②确认 sysinfo 能否读到 WT 子进程 cmdline(UWP 权限);③考虑改用窗口类名
  (WindowsTerminal 的类 CASCADIA_HOSTING_WINDOW_CLASS)兜底。
  **2026-09-16 晚更新**:project_dir 已改为转录 cwd 真实路径(R2),标题匹配①②不再是必败;
  find_session_window 未命中时向 stderr 打印窗口清单与候选 PID(R14),dev 控制台可见

## 交接锚点

- 任何 Agent 接手:先读 [HANDOFF.md](HANDOFF.md)(当前进度/坑/下一步),再从最靠前的 ⬜/🟨 任务继续
- M0 验收标准全文见 [00-REQUIREMENTS](00-REQUIREMENTS.md) 维度⑩
