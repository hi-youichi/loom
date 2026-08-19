# Loom Desk 子代理交互 ↔ Loom 后端 差距审计

> **状态**：Audit，已核实，待排期
> **日期**：2026-08-19
> **范围**：OpenChamber 前端（Loom Desk，`openchamber-feat-dev` 仓）对 Loom 后端（`apps/acp` 扩展 + `agent-core`）子代理相关能力的消费现状。双向差距中的「前端消费侧」——后端侧缺口见 [acp-subagent-contract.md](./acp-subagent-contract.md)
> **方法**：两仓并行全量探索 + 关键结论逐点 grep 复核（通知监听、API 消费方、multirun 启动路径均为零匹配/唯一匹配验证）
> **相关代码**：`apps/acp/src/extensions/agent_profile.rs`、`multi_run.rs`、`goal.rs`、`apps/acp/src/agent.rs`、`agent/agent-core/src/tools/agent/registry.rs`、`agent/agent-core/src/subagent_display.rs`、`agent/agent-core/src/profile.rs`；前端 `packages/ui/src/lib/acp/acp-api.ts`、`stores/useAgentsStore.ts`、`stores/useMultiRunStore.ts`、`components/multirun/`、`components/chat/message/parts/ToolPart.tsx`
> **相关文档**：[acp-subagent-contract.md](./acp-subagent-contract.md)、[25-agent-profile.md](../acp-spec/extensions/25-agent-profile.md)、[14-goal-scheduled-task.md](../acp-spec/extensions/14-goal-scheduled-task.md)、[29-multi-run.md](../acp-spec/extensions/29-multi-run.md)

## 1. 结论速览

| 能力域 | 后端 | 前端 | 状态 |
|---|---|---|---|
| Agent Profile CRUD + subagent 模式 | ✅ `agent_profile.rs` | ✅ `useAgentsStore.ts` | **已对齐** |
| 子代理执行进度基础展示 | ✅ `session/update` 事件流 | ✅ `ToolPart.tsx` TaskToolSummary | **已对齐**（基础级） |
| 子代理会话只读保护 | ✅ 子会话模型 | ✅ `ReadOnlyPromptBanner` + 设置项 | **已对齐** |
| Multi-Run 服务端编排 | ✅ `multi-run/create|cancel|status` | ❌ 客户端自编排 session | **未接入** |
| Goal 目标管理 | ✅ 完整 6 方法 + 进度模型 | ⚠️ API 已封装、UI 零消费 | **UI 全缺** |
| 扩展通知通道 | ✅ 4 类推送 | ❌ 0 处监听 | **未接入** |
| 子代理取消 / 后台转换 / 完成统计 | ✅ `AsyncAgentRegistry` | ❌ 无取消入口、无统计展示 | **未消费** |
| 嵌套子代理链可视化 | ✅ depth 事件格式化 | ❌ 无嵌套树视图 | **未消费** |
| 子代理会话接管（fork 转正） | ✅ `fork_session` | ❌ 仅只读横幅 | **未消费** |

## 2. 已对齐能力（证据）

- **Agent Profile 管理**：前端 `packages/ui/src/lib/acp/acp-api.ts:190` 调 `_loomdesk.dev/agent/list`；`stores/useAgentsStore.ts:337-544` 完整 CRUD，支持 `mode: "subagent"`（`useAgentsStore.ts:103-117`）↔ 后端 `apps/acp/src/extensions/agent_profile.rs:433,488,595,625`。注：[acp-subagent-contract.md](./acp-subagent-contract.md) 缺口清单 #3（「未实现 agent/list」）截至本审计日已落地。
- **子代理进度展示**：`ToolPart.tsx:1351-1416` TaskToolSummary 摘要、`ToolPart.tsx:2681-2691` 实时进度，吃 ACP 标准 `session/update` 通知（`lib/acp/acp-session-store.ts:105`）。
- **「打开子代理会话」交互**：`ToolPart.tsx:1375-1437` task 卡片上的 Open 按钮 → ContextPanel 只读 chat tab（`readOnly: true`、dedupe `session:{id}`、label 为 agent 类型名）；嵌入式/mobile/VS Code 原地 `setCurrentSession` 跳转。注意：交互依赖 `taskSessionId` 三级解析（tool call metadata → part metadata → `<task id=...>` output 解析，`ToolPart.tsx:2301-2340`），Loom ACP 路径下 metadata 均缺失，仅靠 `resolveFallbackTaskSessionId.ts` 时间窗启发式（3s/重试 8s）绑定——即此交互对 Loom 后端处于降级状态，根治依赖 [acp-subagent-contract.md](./acp-subagent-contract.md) P0（metadata 透传）。
- **只读保护**：`ChatContainer.tsx:413-423` ReadOnlyPromptBanner；`useUIStore.ts:662,970,2147` `allowPromptingSubagentSessions` 设置控制。

## 3. 缺口详情（按建议优先级排序）

### GAP-1（P0）：Goal UI 全缺

- **后端**：`_loomdesk.dev/goal/list|get|start|cancel|pause|resume` 六方法（`goal.rs`），进度模型含 `completed_steps`/`total_steps`/`percentage`/`sessions_spawned`，Goal 独立于 connection 持久化运行。规范见 [14-goal-scheduled-task.md](../acp-spec/extensions/14-goal-scheduled-task.md)。
- **前端**：`acp-api.ts:787-802` 已封装全部 6 方法，但**全仓零调用方**（grep 复核：唯一匹配即封装自身）。无 Goal 列表视图、无启动/暂停/取消交互、无进度展示。
- **影响**：Goal 能力域整体不可达；独立运行的 Goal 用户无法感知与管理。
- **方向**：新增 Goal 面板（列表 + 详情 + 进度），消费现有封装即可，无需后端改动。

### GAP-2（P0）：扩展通知通道零消费

- **后端**推送四类通知：`_loomdesk.dev/agent/changed`（`agent_profile.rs`，Created/Updated/Deleted）、`goal/changed`（14 号规范 :420，Started/Paused/Resumed/Cancelled/Completed/Progress/Failed）、`multi-run/changed` + `multi-run/progress`（29 号规范 :379）。
- **前端**：全仓 grep `goal/changed|multi-run/changed|multi-run/progress|agent/changed` **零匹配**。
- **影响**：agent 列表/Goal/Multi-Run 状态变更只能手动刷新；GAP-1 的实时进度也依赖此通道。
- **方向**：在 ACP WebSocket 通知分发层（`acp-session-store.ts` 一带）注册这四类 handler，先落 `agent/changed` → agents store 增量刷新（成本最低）。

### GAP-3（P1）：Multi-Run 走客户端编排，未接后端

- **后端**：`_loomdesk.dev/multi-run/create|cancel|status`（`multi_run.rs:488` 起），服务端编排：每 run 经 `fork_session`（`apps/acp/src/agent.rs:803`）派生独立 ACP session，`MAX_CONCURRENCY=32` 并发控制，断线后状态仍可查询。规范见 [29-multi-run.md](../acp-spec/extensions/29-multi-run.md)。
- **前端**：`MultiRunLauncher.tsx` → `useMultiRunStore.ts:118` `createMultiRun`，实际是前端循环 `opencodeClient.createSession`（`useMultiRunStore.ts:215,254`）逐个建 session。
- **影响**：无并发控制、无统一取消（只能逐个 stop）、无服务端状态查询（`multi-run/status` 未用）；窗口关闭/前端崩溃即失联，重连后无法恢复编排视图。
- **方向**：`createMultiRun` 改调 `_loomdesk.dev/multi-run/create`，UI 状态改由 `multi-run/changed|progress` 通知（依赖 GAP-2）驱动；`MultiRunWindow.tsx` 增加聚合进度与一键取消。

### GAP-4（P2）：子代理控制与完成统计未消费

- **后端**：`AsyncAgentRegistry`（`agent/agent-core/src/tools/agent/registry.rs:170`）——同步超时自动转后台 `mark_background()`（:201）、`cancel()`（:216）、完成统计 `AgentCompletionStats{turn_count, total_tokens, tool_calls_count}`（:16），终态条目保留上限 `MAX_TERMINAL_ENTRIES=50`（:75）。
- **前端**：无取消运行中子代理的 UI；TaskToolSummary 无统计展示（后端 explore 报告中的 `turn_count/total_tokens/tool_calls_count` 字段前端未取用）。
- **影响**：失控子代理只能等超时；成本不可见。
- **方向**：TaskToolSummary 加取消按钮（需后端补 ACP 层 cancel 方法暴露，见下「后端配套」）+ 统计徽标。

### GAP-5（P2）：嵌套子代理链不可视

- **后端**：`SubagentDisplay`（`agent/agent-core/src/subagent_display.rs:5`）支持 depth 嵌套事件格式化；`max_sub_agent_depth` 默认 3（`agent/agent-core/src/profile.rs:1072`，env `MAX_SUB_AGENT_DEPTH` 可调）。
- **前端**：无按 depth 的嵌套树/缩进视图，子代理的子代理在 UI 上与平级无异。
- **影响**：多层委派任务不可读。
- **方向**：tool call 渲染按嵌套层级缩进 + 展开折叠（部分依赖 [acp-subagent-contract.md](./acp-subagent-contract.md) 的 metadata/parentID 落地）。

### GAP-6（P3）：子代理会话无法接管

- **后端**：`fork_session`（`apps/acp/src/agent.rs:803`）可从子代理会话派生可写会话（继承配置/MCP/模型/agent mode）。
- **前端**：子代理会话只有只读横幅，无「接管/转正」入口。
- **方向**：子代理会话视图加「Fork 继续对话」按钮，调 session fork 后跳转新会话。

## 4. 后端配套缺口（顺带记录）

以下为实现 GAP-4/5 时需要后端侧补的口子（与 [acp-subagent-contract.md](./acp-subagent-contract.md) 的缺口互不重叠）：

- 运行中子代理的取消缺少 ACP 扩展方法暴露（`AsyncAgentRegistry.cancel` 仅 engine 内部使用）。
- 完成统计（`AgentCompletionStats`）未透传到 tool call output / `session/update`。

## 5. 落地顺序建议

1. **P0**：GAP-2 通知分发基建（`agent/changed` 先行）→ GAP-1 Goal 面板（纯消费既有 API + 通知）。
2. **P1**：GAP-3 Multi-Run 迁移服务端编排（通知基建复用）。
3. **P2**：GAP-4 取消 + 统计（含后端配套）、GAP-5 嵌套视图。
4. **P3**：GAP-6 fork 接管。
