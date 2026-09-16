# HANDOFF 交接快照

> 任何 Agent/人接手前必读(顺序:AGENTS.md → WORKFLOW.md → 本文件 → 03-TASKS.md)
> 维护规则:每完成一个任务或结束一次会话,更新本文件

## 当前状态(2026-09-16)

- **阶段**:阶段 3(实施)进行中,进度 **T0–T6 ✅(7/14)**
- ✅ T0 环境 / T1 骨架 / T2 存储层 / T3 ZCode 适配器 / T4 Claude Code 适配器(A2 对账,01-RESEARCH §8)
- ✅ T5 GLM Provider(真实 API 调用通过,凭据自动发现自 claude-menu suppliers.json)
- ✅ T6 hooks 链路:e2e 实测通过(3 个真实事件捕获/卸载还原语义等价/Drop 守卫防残留)
- 关键代码:src-tauri/src/{store/,collector/{mod,zcode,claude_code,hook_events}.rs,provider/{mod,glm}.rs,../hook-bridge/hook-bridge.js}

## 下一步

**T7 状态聚合器**:三级融合(hooks 事件 > 文件/usage 启发式 > 进程枚举)+ 看门狗(working>5min 无
活动回 idle)+ error 判定(Notification 消息限流关键词正则/ZCode error_type/额度 100%)+
岛收缩态聚合逻辑(任一 error→红等);同时在此层落地 T5 的 5min 定时刷新调度与失败降级(最近快照)。
进程枚举兜底需加 sysinfo crate。数据输入:scan_sessions/collect_usage(HookEvent 窗口近 N 秒事件)。
环境提醒:cargo 带 RUSTUP_HOME/CARGO_HOME/PATH,外网 HTTPS_PROXY,Bash 显式 cd /f/AgentTracker。

## 关键背景(新接手者必读)

1. 本工具是**旁路观测台**,五条红线见 AGENTS.md,任何实现决策不得违反
2. ZCode 适配器 = 只读 `~\.zcode\cli\db\db.sqlite` 的 `model_usage` 表(勘察结论见 01-RESEARCH §1)
3. Claude Code 适配器 = 解析 `~\.claude\projects\**\*.jsonl` + hooks 事件文件(协议 02-DESIGN §4)
4. GLM 额度 = Monitor API(`/api/monitor/usage/quota/limit`,裸 key Authorization),响应字段见 01-RESEARCH §7
5. hooks 官方文档站本机不可达 → T6 第一步先装诊断 hook 实测 stdin 字段
6. 调研原始材料(竞品 README/GLM 源码/官方文档摘录)在 `E:\AIAgentTemp\AgentTracker-research\`

## 踩坑记录

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
