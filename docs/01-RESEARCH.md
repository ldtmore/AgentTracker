# AgentTracker 调研报告

> 状态:**调研中**(阶段 2) | 开始:2026-09-16
> 目的:按 [00-REQUIREMENTS.md](00-REQUIREMENTS.md) 逐项找轮子,产出 🟢直接依赖 / 🟡借鉴实现 / 🔴必须自研 三分清单
> 注:早期竞品调研(2026-09-16 第一轮)存档于 E:\AIAgentTemp\AgentTracker-research\

## 1. ZCode 本机数据勘察 ✅(2026-09-16,决定性成果)

### 结论:ZCode 适配器可**纯只读**实现,无需 hooks,数据齐全度超过 Claude Code

| 数据源 | 路径 | 内容与价值 |
|---|---|---|
| **SQLite 库** | `~\.zcode\cli\db\db.sqlite`(WAL) | 核心数据源。关键表:`model_usage`(每模型调用一行,496 行实测)、`turn_usage`(82)、`tool_usage`(462)、`session`(会话元数据) |
| 模型 IO 转录 | `~\.zcode\cli\rollout\model-io-sess_*.jsonl` | 完整请求/响应体(含 headers),可作为 SQLite 的补充校验源 |
| 套餐缓存 | `~\.zcode\v2\coding-plan-cache.json` | ZCode 原生内置 GLM 套餐支持:`builtin:bigmodel-coding-plan`(国内)/`builtin:zai-coding-plan`(国际)等 4 个套餐条目 → 证实 GLM 查询链路成熟 |
| 桌面设置 | `~\.zcode\v2\setting.json` | 仅 UI 偏好,无关采集 |

### `model_usage` 表关键字段(实测)

- **用量**:`input_tokens / output_tokens / reasoning_tokens / cache_creation_input_tokens / cache_read_input_tokens / computed_total_tokens`
- **标识**:`provider_id / model_id / agent / mode / task_type / session_id / turn_id`
- **性能**:`duration_ms / time_to_first_token_ms`(CC Switch 式时长统计的现成数据!)
- **状态/错误**(状态监控直接可用):`status / error_type / error_code / error_message / retry_count / cancelled_by_user / context_exceeded`

### 采集方案定论(写入 02-DESIGN 的依据)

- 🟢 直接依赖:以 `node:sqlite`/`rusqlite` **只读打开** db.sqlite(WAL 模式支持并发读,不干扰 ZCode 运行)
- 🟢 会话状态:由 `model_usage.started_at/completed_at` + `session.time_updated` 推断(工作/空闲),错误由 `error_type` 判定
- 🟢 增量采集:按 `model_usage.id`/`started_at` 水位增量拉取,天然满足红线③(顺序无关/回溯补录)
- 🔴 需自研:仅"水位管理与聚合入库"薄层
- ⚠️ 风险:ZCode 升级可能改 Schema → 解耦层容忍未知列,按列名白名单读取

## 2. Claude Code 数据源(已验证)

- `~\.claude\projects\**\*.jsonl`:assistant 消息含 `message.model`(glm-5.3)+ `message.usage`(input/output/cache_read/cache_creation + thinking)
- hooks 机制:`PreToolUse/PostToolUse/Stop/Notification/SessionStart/SessionEnd` 等,`~\.claude\settings.json` 配置(所有者已在用 Notification/Stop hook,链路已验证)
- 对账基准:ccusage(18.5k⭐,Rust)

## 3. GLM Coding Plan 额度(已验证)

- 官方 Monitor API:`GET {open.bigmodel.cn|api.z.ai}/api/monitor/usage/quota/limit`,Header `Authorization:<key>`(无 Bearer,key=ANTHROPIC_AUTH_TOKEN 同值)→ 5h%/周%/套餐档/重置时间
- 积分制:5h 窗口动态刷新(消耗起 5h 后重置);周窗口按下单日 7 天周期;glm-5.3/glm-5-turbo 高峰期(工作日 14:00–18:00 UTC+8)×3 系数
- 参考实现:glm-quota-line(33⭐)、ecerutti/glm-usage-monitor(接口三件套已列明)

## 4. Tauri 生态轮子盘点 ✅(2026-09-16)

| 需求 | 轮子 | 结论 |
|---|---|---|
| 文件监听 | notify-rs/notify(3452⭐,2026-09 仍活跃) | 🟢 直接依赖 |
| SQLite | rusqlite(bundled feature) | 🟢 直接依赖(自库读写 + ZCode 库只读均用它) |
| 毛玻璃 | tauri-apps/window-vibrancy(1037⭐,活跃;Acrylic/Mica) | 🟢 直接依赖 |
| 托盘/全局快捷键 | Tauri 2 内置(tray-icon feature) | 🟢 直接依赖 |
| 开机自启 | tauri-plugin-autostart(官方 plugins-workspace,1815⭐) | 🟢 直接依赖(M0 预留接口,默认关) |
| 置顶/无边框/透明窗 | Tauri 2 内置(always_on_top/decorations/transparent) | 🟢 直接依赖 |
| 窗口激活(跳转) | Windows SetForegroundWindow(windows-rs crate) | 🟢 直接依赖 |
| 图表 | ECharts(M1 报表页) | 🟢 直接依赖 |

## 5. 灵动岛 UI 实现参考 ✅(2026-09-16)

- 🔴 **Tauri 生态无现成灵动岛项目**(gh 搜索 "tauri island"/"tauri notch" 零结果)→ 岛 UI 自建
- 🟡 交互设计借鉴:open-island-windows(C#,19⭐)——顶部贴边收起/hover 展开/点击会话卡的交互范本(借鉴行为,不抄码)
- 🟡 视觉借鉴:iOS 灵动岛经典胶囊形态 + DynamicWin(599⭐)的 Windows 适配经验

## 6. Claude Code hooks 协议 ✅ 策略变更(2026-09-16)

- 官方文档站(code.claude.com / docs.anthropic.com)本机网络不可达(直连与代理均超时)
- **决定:hooks stdin 细节改为实施期实测验证**——所有者本机已配置 Notification/Stop hooks(结构:matcher/hooks[type=command,command,timeout,async] 已见),T6 任务先写诊断 hook 打印真实 stdin JSON 再定协议,比文档更可靠

## 7. GLM Monitor API 响应格式 ✅(2026-09-16,源码级确认)

来自 glm-quota-line 源码(src/core/quota/fetch.js + parse.js,已存 E:\AIAgentTemp\AgentTracker-research\):

- 请求:`GET {base}/api/monitor/usage/quota/limit`,Header `Authorization: <裸key>`(无 Bearer)、`Accept: application/json`
- 响应字段(多口径容错):`usage / remaining / currentValue / total / percentage(历史遗留字段=已用%) / nextResetTime(毫秒时间戳)`
- 限流判定正则(429 场景):`rate limit|too many requests|限流|频率|过于频繁|稍后再试`
- token 级用量(报表用):`/api/monitor/usage/model-usage?startTime&endTime`(ecerutti README)

## 8. T4 对账记录(2026-09-16,重要口径结论)

对账三方(同一台机器、同一数据):

| 工具 | input | output | cache_read | total | 口径 |
|---|---|---|---|---|---|
| **AgentTracker(T4 实现)** | 1,295,911 | 379,214 | 26,300,032 | 27,975,157 | assistant 行,messageId(+requestId)去重保留最大快照 |
| ccusage(Rust 版) | 1,495,477 | 389,518 | 26,572,544 | 28,457,539 | 同上去重 + 额外纳入非 assistant 源 |
| better-ccusage(TS 版) | 75,329,127 | 884,293 | 98,125,376 | 174,338,796 | 多源(Claude Code+ZCode 混合),不可直接比 |

**结论与依据**:

1. 本机 1514 条 assistant 行实测**无一条带 requestId**(0/1514),组合键退化为 messageId,
   我们与 ccusage 在 assistant 行源上的口径完全一致(better-ccusage 的 `createUniqueHash` 同款逻辑)
2. 残差 1.7%(482K)来源:本机数据存在 **`cost-state` 行类型**(56 行,会话级模型用量累计快照,
   含 glm-5.3-flash 等非 assistant 路径的用量),ccusage Rust 新版将其纳入;两个参考工具互不一致
   证明"标准口径"本身不唯一
3. **本地解析只是近似,供应商侧数字才是最终裁判**——T5 接入 GLM Monitor API 后,
   用官方用量对账作为 A3 验收的黄金标准
4. 关键去重知识(后续维护必读):JSONL 同一 assistant 消息平均重复 ~3 次(流式快照/会话恢复复制),
   按 messageId 去重是底线;`<synthetic>` 模型行是本地合成消息,usage 为 0,过滤

---

## 调研日志

| 日期 | 进展 |
|---|---|
| 2026-09-16 | §1 ZCode 勘察完成(决定性);§2/§3 基于早期调研归档 |
| 2026-09-16 | §4 轮子盘点完成(全🟢);§5 确认 Tauri 无现成岛→自建;§6 hooks 改实测策略;§7 GLM 响应格式源码级确认——**阶段 2 调研收官** |
