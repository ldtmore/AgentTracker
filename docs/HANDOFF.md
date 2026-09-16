# HANDOFF 交接快照

> 任何 Agent/人接手前必读(顺序:AGENTS.md → WORKFLOW.md → 本文件 → 03-TASKS.md)
> 维护规则:每完成一个任务或结束一次会话,更新本文件

## 当前状态(2026-09-16 晚,第二次会话更新)

- **阶段**:阶段 3(实施)进行中,进度 **T0–T9、T11 ✅;T10 🟨(ZCode 跳转 ✅,WT 未命中搁置);
  T12 ⏸(所有者指示:先完成代码细节调整);T13 ⬜**
- ✅ T8 岛壳/T9 展开面板/T10 点击跳转(部分)/T11 设置页——验收记录见 03-TASKS.md 对应条目
- git:T0–T11 的 4 个提交 + 本次审查修复提交,均已推送 GitHub(ldtmore/AgentTracker,main)
- 本次会话:①需求完成度核对——M0 功能代码全部就绪,缺口仅"阈值可配未生效(R5)"+WT 跳转+T12 打包;
  ②全量代码审查(14 项发现)并按所有者勾选修复 12 项(R1–R5/R7–R12/R14;R6 文件级增量、
  R13 CSP 明确不做):CC 模型兜底回填/转录 cwd 真实路径/进程探测改命令行判定/水位 60s 安全余量/
  hook 事件审计落库/死代码清理/hook 偏移免写/阈值接线+校验/收缩态"累计"前缀/模板残留/跳转诊断日志。
  验证:cargo 单测 14/14 + 真实数据集成 3/3 + 零警告 + npm build 通过
- 注意:项目实际路径为 F:\MyProjectRepository\AgentTracker(旧文档中 F:\AgentTracker 为历史写法)

## 下一步

1. 所有者 tauri dev 屏幕复核两处 UI(踩坑铁律:UI 改动以真实渲染为验收):
   收缩态 token 显示"累计"前缀;阈值变色(设置页改阈值保存后需重启应用生效)
2. 恢复 T12(release 构建 + 性能实测 + 红线回归 + 便携 zip)
3. T10 WT 跳转:R2 已修(project_dir 为真实路径),按待议区线索 + R14 诊断输出重启调试
环境提醒:cargo 带 RUSTUP_HOME/CARGO_HOME/PATH,外网走本机代理 127.0.0.1:6478,
Bash 显式 cd 到项目目录。

## 关键背景(新接手者必读)

1. 本工具是**旁路观测台**,五条红线见 AGENTS.md,任何实现决策不得违反
2. ZCode 适配器 = 只读 `~\.zcode\cli\db\db.sqlite` 的 `model_usage` 表(勘察结论见 01-RESEARCH §1)
3. Claude Code 适配器 = 解析 `~\.claude\projects\**\*.jsonl` + hooks 事件文件(协议 02-DESIGN §4)
4. GLM 额度 = Monitor API(`/api/monitor/usage/quota/limit`,裸 key Authorization),响应字段见 01-RESEARCH §7
5. hooks 官方文档站本机不可达 → T6 第一步先装诊断 hook 实测 stdin 字段
6. 调研原始材料(竞品 README/GLM 源码/官方文档摘录)在 `E:\AIAgentTemp\AgentTracker-research\`

## 踩坑记录

- **项目迁移目录坑(2026-09-16 验证实测)**:target/ 构建缓存嵌旧绝对路径(F:\AgentTracker),
  目录变更后 tauri 构建脚本报"系统找不到指定的路径"(指向旧盘符路径)——`cargo clean`
  全量重建即可恢复(约 6 分钟)
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
