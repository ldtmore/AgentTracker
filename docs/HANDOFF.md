# HANDOFF 交接快照

> 任何 Agent/人接手前必读(顺序:AGENTS.md → WORKFLOW.md → 本文件 → 03-TASKS.md)
> 维护规则:每完成一个任务或结束一次会话,更新本文件

## 当前状态(2026-09-16)

- **阶段**:阶段 3(实施)进行中,进度 **T0–T7 ✅(8/14)——数据+状态层全部就绪**
- ✅ T0–T6 见前次记录(数据层五件套:存储/ZCode/ClaudeCode/GLM/hooks)
- ✅ T7 状态聚合器:state/{mod,service}.rs;单测 13/13+集成 7/7;
  e2e 实测 46 会话/岛 AnyError(额度100%标红)/ZCode 47M tokens;
  关键教训:hooks 事件窗口(30min)必须>看门狗(5min)
- git:首次提交已推送 GitHub(ldtmore/AgentTracker,main);T7 之后的提交待所有者指示

## 下一步

**T8 岛 UI-壳**:tauri.conf.json 主窗改岛形态(decorations:false/always_on_top/skip_taskbar/
transparent/resizable:false/shadow:false)+ window-vibrancy Acrylic + 默认顶部居中(计算
workArea)+ 拖拽记忆(app_settings 存坐标)+ 托盘菜单;lib.rs setup 里 spawn 后台线程 10s tick
调 Aggregator,emit "island-snapshot" 事件给前端。
之后 T9 岛 UI-内容(React 组件消费快照)。
环境提醒:cargo 带 RUSTUP_HOME/CARGO_HOME/PATH,外网 HTTPS_PROXY,Bash 显式 cd /f/AgentTracker。

## 关键背景(新接手者必读)

1. 本工具是**旁路观测台**,五条红线见 AGENTS.md,任何实现决策不得违反
2. ZCode 适配器 = 只读 `~\.zcode\cli\db\db.sqlite` 的 `model_usage` 表(勘察结论见 01-RESEARCH §1)
3. Claude Code 适配器 = 解析 `~\.claude\projects\**\*.jsonl` + hooks 事件文件(协议 02-DESIGN §4)
4. GLM 额度 = Monitor API(`/api/monitor/usage/quota/limit`,裸 key Authorization),响应字段见 01-RESEARCH §7
5. hooks 官方文档站本机不可达 → T6 第一步先装诊断 hook 实测 stdin 字段
6. 调研原始材料(竞品 README/GLM 源码/官方文档摘录)在 `E:\AIAgentTemp\AgentTracker-research\`

## 踩坑记录

- **UI 三连坑(T8 实战)**:①写前端文件路径勿多一层(曾误写 src/src/App.tsx 导致 vite 一直服务模板——Write 成功≠路径正确,**UI 改动必须以屏幕真实渲染为验收**);②window-vibrancy/Acrylic 是**窗口级**效果,整个矩形窗口变磨砂灰,胶囊形态必须"窗口全透明+CSS 自绘背景"(依赖保留未用);③tauri dev 用 TaskStop 后 agenttracker.exe 与 vite 可能残留并占 1420 端口,重启 dev 前先 taskkill + 清端口
- **Claude Code JSONL 数据知识(T4 实测)**:同一 assistant 消息平均重复 ~3 次(流式快照/会话恢复复制),必须按 messageId(+requestId)去重并保留用量最大快照;`<synthetic>` 行是本地合成消息(usage 全 0)须过滤;`cost-state` 行是会话级累计快照(ccusage 纳入、我们没有,对账差 1.7% 的来源);本机数据无 requestId 字段;**集成测试勿断言"增量采集为空"**(活跃会话在写入,竞态必挂,容忍 ≤5 行)——对账详情见 01-RESEARCH §8
- **参考工具优先**:遇到解析/口径问题先看调研存档 E:\AIAgentTemp\AgentTracker-research\(better-ccusage/ccusage/glm-quota-line 源码),别自己盲试变体
- **Rust 测试要点**:mod tests 所在文件必须在父 mod.rs 里声明(`pub mod zcode;`),否则整文件不参与编译且无任何警告;模型名比较一律小写化(真实数据 GLM-5.3/glm-5.3 混用);rusqlite 0.40 的 Error 无 io 变体,统一用 anyhow
- **🌐 所有外网访问(rustup/crates.io/npm/GitHub 下载)走所有者本机代理 `127.0.0.1:6478`**(2026-09-16 所有者指示,速度关键):curl 用 `-x 127.0.0.1:6478`,或设 `HTTPS_PROXY/HTTP_PROXY`
- **安装顺序坑**:rustup-init 检测不到 MSVC 时会自动往 C 盘装 VS Build Tools——必须先装 VS(D:\VSBuildTools)再跑 rustup-init
- Anthropic 文档站(code.claude.com/docs.anthropic.com)直连与 Jina 代理均超时 → hooks 细节走实测
- gh search 的 JSON 字段名用 `language`(不是 primaryLanguage);`gh repo view` 用 `stargazerCount`
- 本机 Node 24 内置 `node:sqlite`,读 ZCode 库用它即可(readOnly 打开,WAL 并发读安全)
- 所有者 D 盘为"软件安装盘":每软件一目录平铺(D:\Git、D:\Python314…);本项目工具链布局 D:\Rust\{rustup,cargo} + D:\VSBuildTools(2026-09-16 确认)

## 所有者偏好(交互层面)

- 简体中文交流;结构化表格+emoji;先计划后编码;重大变更先确认
- 修改>3 文件或>10 行代码先列计划(本看板任务已视同获批计划,但看板外变更仍需确认)
- 不自动 git commit,等确认
