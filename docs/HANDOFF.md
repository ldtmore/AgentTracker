# HANDOFF 交接快照

> 任何 Agent/人接手前必读(顺序:AGENTS.md → WORKFLOW.md → 本文件 → 03-TASKS.md)
> 维护规则:每完成一个任务或结束一次会话,更新本文件

## 当前状态(2026-09-17,第三次会话更新)

- **阶段**:阶段 3(实施)进行中,**M0 已收尾(2026-09-17);M1 进行中**。
  进度 **T0–T9、T11 ✅;T10 🟨(ZCode ✅,WT 搁置);T12 ⏸(dev 可验三件待所有者;
  打包动作随正式发布);T13 ⬜;M1-1 报表页/M1-3 Codex 调研/M1-6 贴边隐藏完成,
  M1-7 审查修复+文档回写完成(2026-09-17);M1-2 ⬜(等所有者清单)**
- 已知搁置:所有者暂记了一批功能/统计/展示的待优化项(见 03-TASKS 待议区),
  后续统一优化修改;有新发现随时补录
- ✅ T8 岛壳/T9 展开面板/T10 点击跳转(部分)/T11 设置页——验收记录见 03-TASKS.md 对应条目
- git:M1-1/M1-6/M1-7 代码与文档回写已提交推送(aa7a0b3,2026-09-17);
  **2026-09-17 项目更名 AgentTrackerIsland,全量标识更正随本次会话提交**
- 第三次会话(2026-09-17):**全量需求↔代码审查**(发现 P1 凭据回退/P2 增量去重口径/
  P3 进程误判 + 一批文档未回写),按所有者指示**修复代码问题后按代码回写文档**:
  ①GLM 凭据留空回落发现链+来源展示;②用量幂等键冲突取最大快照;③hook-bridge
  进程误判排除;④calc_percent 不冒充百分比;⑤移除 online 死状态(六处同步);
  ⑥陈旧注释修正;02-DESIGN 七处回写(§1 技术栈/§2.1 trait/§2.2 凭据链/§2.3 状态机/
  §3 清理/§4 hook 协议/§5 托盘),看板+HANDOFF 同步。
  验证:cargo test **19/19**(新增 upsert 快照测试)+ npm run build 通过
- 注意(2026-09-17 更名):项目由 AgentTracker 更名为 **AgentTrackerIsland**,实际路径
  F:\MyProjectRepository\AgentTrackerIsland(旧文档中 F:\MyProjectRepository\AgentTracker /
  F:\AgentTracker 均为更名前写法);标识符同步切换:GitHub 仓库 ldtmore/AgentTrackerIsland、
  npm/cargo 包名 agenttrackerisland、lib 名 agenttracker_island_lib、Tauri identifier
  com.agenttrackerisland.app、自库 %APPDATA%\com.agenttrackerisland.app\agenttrackerisland.db、
  事件目录 %LOCALAPPDATA%\AgentTrackerIsland\events——旧路径数据留在原地未迁移,
  更名后需在设置页**重新安装 hooks**(旧 hook-bridge 仍写旧事件目录);
  调研存档 E:\AIAgentTemp\AgentTracker-research\ 为更名前目录,保留原名

## 下一步

1. **M1 进行中**(2026-09-17 启动,顺序:P1 报表→P2 搁置优化→P3 Codex→P4 双主题
   →P5 形态视进度;Anthropic 额度移 M2,NSIS 归发布):**M1-6 贴边自动隐藏已验收通过**
   (身份色体系/等宽分段/扇形辐射/额度弧线/Agent 选择与采集开关/宽度自适应);
   **M1-1 报表页代码完成+实例内自验通过**,待所有者过目;**M1-3 Codex 源码级调研完成**
   (01-RESEARCH §10,适配器待真实样本);**M1-7 审查修复+文档回写完成**(2026-09-17,
   P1–P3 修复与 02-DESIGN 七处回写,详见看板);M1-2 等所有者搁置问题清单
2. 所有者 M0 遗留 dev 补验(可并入日常使用):①A1 waiting 场景(hooks+CC 等待输入→岛琥珀);
   ②拖拽记忆;③托盘各菜单项;④本轮修复项抽查(GLM 凭据留空保存→重启→额度仍正常)
3. T10 WT 跳转可随时按待议区线索+R14 诊断日志调试;**构建打包仅当所有者明确宣布
   "正式对外发布"时执行**(WORKFLOW 构建打包纪律)
环境提醒:cargo 带 RUSTUP_HOME/CARGO_HOME/PATH,外网走本机代理 127.0.0.1:6478,
Bash 显式 cd 到项目目录。

## 关键背景(新接手者必读)

1. 本工具是**旁路观测台**,五条红线见 AGENTS.md,任何实现决策不得违反
2. ZCode 适配器 = 只读 `~\.zcode\cli\db\db.sqlite` 的 `model_usage` 表(勘察结论见 01-RESEARCH §1)
3. Claude Code 适配器 = 解析 `~\.claude\projects\**\*.jsonl` + hooks 事件文件(协议 02-DESIGN §4)
4. GLM 额度 = Monitor API(`/api/monitor/usage/quota/limit`,裸 key Authorization),响应字段见 01-RESEARCH §7
5. hooks 官方文档站本机不可达 → T6 第一步先装诊断 hook 实测 stdin 字段
6. 调研原始材料(竞品 README/GLM 源码/官方文档摘录)在 `E:\AIAgentTemp\AgentTracker-research\`(2026-09-17 项目更名前存档,路径保留原名)

## 踩坑记录

- **DPI 逻辑/物理坐标坑(M1-6)**:窗口尺寸(tauri.conf)是逻辑像素,事件/显示器坐标是
  物理像素,缩放 ≠100% 时直接混用会导致贴边判定偏移、隐藏标签"飘"到屏中间——
  贴边几何全链路统一逻辑坐标(phys_to_logical/monitor_logical 换算,落位再转回物理);
  另:程序化 set_position 产生的 Moved 事件与用户拖拽不可区分,必须走动画标记+落点
  消费+滑动代数三重防护,启动定位也要走同一通道
- **drag-region 权限坑(M1-6 实测)**:`data-tauri-drag-region` 底层走 `start_dragging`
  命令,而 `core:window:default` 权限集**不含** `allow-start-dragging`(只有只读类)——
  权限被拒时前端静默无反应,T8 起拖拽从未生效直到 M1-6 验收才暴露。修复:capabilities
  加 `core:window:allow-start-dragging`。教训:涉及交互的新能力,验收必须真实操作一遍
- **程序化移动 vs 拖拽事件坑(M1-6)**:代码里 `set_position` 会触发 Moved 事件,与用户
  拖拽不可区分——贴边吸附用"动画标记 animating + 落点 programmed + 滑动代数 SLIDE_GEN"
  三重机制屏蔽自身事件,启动定位也必须走同一通道,否则会自触发吸附循环
- **项目迁移目录坑(2026-09-16 验证实测)**:target/ 构建缓存嵌旧绝对路径(F:\AgentTracker),
  目录变更后 tauri 构建脚本报"系统找不到指定的路径"(指向旧盘符路径)——`cargo clean`
  全量重建即可恢复(约 6 分钟);2026-09-17 更名迁移到 F:\MyProjectRepository\AgentTrackerIsland
  后同样执行了 cargo clean 重建
- **UI 三连坑(T8 实战)**:①写前端文件路径勿多一层(曾误写 src/src/App.tsx 导致 vite 一直服务模板——Write 成功≠路径正确,**UI 改动必须以屏幕真实渲染为验收**);②window-vibrancy/Acrylic 是**窗口级**效果,整个矩形窗口变磨砂灰,胶囊形态必须"窗口全透明+CSS 自绘背景"(依赖保留未用);③tauri dev 用 TaskStop 后 agenttrackerisland.exe 与 vite 可能残留并占 1420 端口,重启 dev 前先 taskkill + 清端口
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
- **构建打包是最后一步**:仅当所有者明确提出"打包构建正式对外发布"时才执行
  (release/zip/安装包);其余阶段一律本机调试开发 + GitHub 代码提交,任何 Agent
  不得主动 `tauri build`。cargo test/npm run build 等验证性构建不受限。
  (2026-09-17 指示,详见 WORKFLOW 构建打包纪律)
