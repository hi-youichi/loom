# Loom ACP 子代理契约设计

> **状态**：Draft，待评审
> **日期**：2026-08-19
> **范围**：将 Loom 已有的 in-process 子代理执行引擎升级为 ACP 一等公民 —— tool call metadata 透传、`parentID` 子会话、`agent/list` + subagent 模式、级联删除，对齐 OpenChamber 前端已有的 task/子会话契约
> **相关代码**：`agent/agent-core/src/tools/agent/`（mod.rs、runner.rs、worktree.rs、registry.rs、build_config.rs）、`apps/acp/src/stream_bridge.rs`、`apps/acp/src/session_repository.rs`、`apps/acp/src/agent.rs`、`apps/acp/src/agent_registry.rs`、`apps/acp/src/extensions/agent_profile.rs`
> **相关文档**：[02-session-lifecycle.md](../acp-spec/02-session-lifecycle.md)、[05-session-update.md](../acp-spec/05-session-update.md)、[acp-cli-extension.md](./acp-cli-extension.md)、[openchamber-projects-integration.md](./openchamber-projects-integration.md)

## 1. 背景与问题

Loom 的 `agent` 工具（`agent/agent-core/src/tools/agent/`）已经实现了完整的子代理执行引擎：profile 解析、模型继承与覆盖、worktree 隔离、后台注册表（`AsyncAgentRegistry`）、深度限制（默认 3 层）、事件转发进父会话流。子代理运行时拥有独立 `thread_id`（`sub-{parent}-{agent}-{depth}`），checkpoint 落入同一 SQLite 存储。

但从 OpenChamber 前端的视角看，这些子代理是**不可观测的**：ACP wire 协议上没有任何信号把「父会话里的一个 `agent` tool call」和「一个子会话」关联起来。前端契约（见 §2.2）在 ACP 路径下全部退化为启发式。

### 1.1 缺口清单（逐文件核实，2026-08-19）

| # | 缺口 | 位置 | 前端后果 |
|---|---|---|---|
| 1 | tool call 不携带 metadata | `apps/acp/src/stream_bridge.rs` 全文无一处设置 `ToolCallContent.metadata` | `explicitTaskSessionId` 永远为空，task ↔ 子会话绑定退化为 3s/8s 时间窗启发式（`resolveFallbackTaskSessionId.ts`） |
| 2 | 会话无 `parentID` | `apps/acp/src/session_repository.rs` 的 `acp_sessions` 表无该列 | 子代理 thread 的 checkpoint 以孤立 session 漏进 `session/list`；侧边栏无法建树 |
| 3 | 未实现 `agent/list`、SessionMode 无 subagent 标记 | `apps/acp/src/agent.rs` 无该方法；`apps/acp/src/agent_registry.rs:47` 仅 id/name/description | 前端 agent 选择器把所有 profile 当 primary；subagent 不应出现在主对话选择列表 |
| 4 | 无级联删除 | `apps/acp/src/session_repository.rs:206` `delete_all` 只删单 session | 删父留孤儿 |

## 2. 参考实现与契约

### 2.1 opencode 后端（参考实现，`packages/opencode/src/`）

- **task 工具**（`tool/task.ts`）：参数 `description`/`prompt`/`subagent_type`/`task_id`（续用旧子会话）/`background`。执行时创建 `parentID` 子会话，`ctx.metadata({parentSessionId, sessionId, model})` 是前端绑定的第一数据源；输出包装为 `<task id=子会话ID state=...>` 文本作为 fallback 解析源
- **深度限制**：沿 `parentID` 链上溯计数，默认 `subagent_depth: 1`
- **权限派生**（`agent/subagent-permissions.ts`）：子会话 permission = 父 deny 规则 + `external_directory`，再默认追加 `todowrite:deny` 和 `task:deny`（除非 subagent 配置显式允许）→ 默认禁止嵌套派生
- **级联删除**（`session/session.ts`）：`remove()` 递归删所有 children
- **agent 注册表**（`agent/agent.ts`）：`mode: subagent|primary|all`；默认 agent 不能是 subagent；UI 选择器过滤 `mode !== "subagent"`
- **后台子代理**（实验）：`BackgroundJob` 完成/更新时向父会话注入 synthetic message；运行中 task 可 `extend` 追加上下文；父中断级联 cancel 子会话

### 2.2 OpenChamber 前端契约（`packages/ui/src/`）

- `toolHelpers.ts`：`task` 工具名 → 识别为子代理派生点
- `ToolPart.tsx` 三级解析 `taskSessionId`：① tool call metadata → ② part metadata → ③ 从 tool output 解析（`<task id=...>`）；兜底走 `resolveFallbackTaskSessionId.ts`（`parentID === 当前会话` + 创建时间窗 3s/重试 8s + live 状态消歧，多候选拒绝猜测）
- `SessionSidebar.tsx`：按 `parentID` 建树，子会话标记 `isSubtaskSession`
- `session-actions.ts`：删父会话时级联（子 404 视为成功）；发消息时对会话子树内 subagent 的 pending question 做 dismiss
- ACP 路径下 `acp-session-actions.ts` 的 `_parentID` 被忽略，父子关系全靠后端建立、前端事后发现

**结论**：OpenChamber 前端已按 opencode 的四类信号（metadata、`<task>` 输出、parentID、级联删除）设计完毕，Loom 只需在 ACP 层补齐契约，无需改前端。

## 3. 方案（按优先级）

### 3.1 P0 — tool call metadata 透传

最小改动、收益最大：让前端第一级解析生效，启发式退役。

- `runner.rs` 已把子代理事件经 `any_stream_event_sender` 序列化进父流；需要把 `agent_id`、父 `thread_id` 带进子代理转发事件的 envelope（避免依赖全局序号反查）
- `stream_bridge.rs` 在 `ToolStart`/`ToolEnd` 构造 `ToolCallUpdate` 时，对 `TOOL_AGENT` 调用注入 ACP 原生 `ToolCallContent.metadata`：

```json
{
  "parentSessionId": "sess_parent",
  "subSessionId": "sub-root-dev-0-42",
  "agent": "dev",
  "model": "glm-4.7"
}
```

- 同时在同步完成的 tool output 文本里追加 `<task id="{agent_id}" state="completed">` 包装，对齐 opencode 的 fallback 解析源（前端第三级解析零改动可用）

### 3.2 P1 — `parentID` 子会话

- **schema**：`session_repository.rs` 照 `ensure_title_column` 模式加列：

```sql
ALTER TABLE acp_sessions ADD COLUMN parent_id TEXT;
CREATE INDEX idx_acp_sessions_parent ON acp_sessions(parent_id);
```

- **注册时机**：`agent` 工具 spawn 前注册子 session —— `session_id` 复用 `agent_id`，`thread_id` 用现有 `sub_thread_id`，`parent_id` = 父 thread_id
- **分层约束**：agent-core 不依赖 apps/acp；通过 `ToolCallContext` 注入 hook 回调（`on_subagent_session(agent_id, sub_thread_id, parent_thread_id)`），唯一 production 接线点在 `apps/acp/src/stdio_loop.rs::extension_context_for`
- **list**：`agent.rs::SessionInfo` 增加 `parent_id` 字段随 `session/list` 带出；默认列表应排除 subagent 子会话（或前端按 parent_id 折叠）
- **删除**：`delete_all` 先递归查 `WHERE parent_id = ?` 删子树再删自身，事务内完成

### 3.3 P2 — `agent/list` 与 subagent 模式

- `agent/profile::AgentProfile`（agent-core 侧）增加 `mode: primary | subagent | all`（默认 `primary`）；`explore` 等内置 profile 标 `subagent`
- `agent_registry.rs` 实现 ACP `agent/list`：`subagent` 布尔由 `mode` 映射；`agent_profile.rs` 扩展的 profile 编辑面同步补 `mode` 字段
- **权限对齐**：subagent 模式默认 disable `TOOL_AGENT` + `TODO_WRITE`（在 `build_config_from_profile` 的 `builtin_tool_filter` 逻辑，profile 显式 enable 可覆盖）—— 对齐 opencode 的默认 deny 语义，替代单纯依赖 `max_depth` 的递归防护

### 3.4 P3 — 补强（可选）

- background 子代理完成时 push `_loomdesk.dev/*` notification（对齐 opencode BackgroundJob 的 synthetic message），替代前端轮询 `agent_get`
- 深度计数改为沿 `parent_id` 链上溯（`SessionRepository` 查询），支持 opencode 式 `subagent_depth: 1` 语义；当前全局 `ctx.depth` 计数已够用
- task 续用（`task_id` 复用已有子会话）与运行中 extend —— 需求出现后再评估

## 4. 实施顺序与验证

| 阶段 | 改动面 | 验证 |
|---|---|---|
| P0 | `stream_bridge.rs`、`runner.rs` | headless playwright hook WebSocket 抓 `tool_call/update` 帧断言 metadata 字段；OpenChamber task 摘要不再走时间窗 fallback |
| P1 | `session_repository.rs`（+迁移测试）、`agent.rs`、agent-core hook、`stdio_loop.rs` | `session/list` 带 parent_id 建树；删父会话子树级联 404；SQLite 迁移对旧库幂等 |
| P2 | agent-core `profile`、`agent_registry.rs`、`extensions/agent_profile.rs` | `agent/list` 返回 subagent 标记；subagent profile 默认无 `agent`/`todo_write` 工具 |
| P3 | notification 通道 | background 完成后前端收到 push 无需轮询 |

测试遵循 [rust-testing.md](../dev/rust-testing.md)（nextest 优先）；E2E 用 `e2e/` 的 mock-opencode BDD 套件骨架新增 ACP 直连场景（参考 [e2e-bdd.md](../dev/e2e-bdd.md) 的诊断 fixture 抓 wire 帧）。

## 5. 风险与开放问题

- **同步/后台双路径**：`background: true` 时 metadata 只在 `ToolStart` 可用（无 `ToolEnd` 结果），前端需容忍子会话晚绑定 —— 现有 fallback 逻辑天然覆盖，但应在 P0 的 metadata 里带上 `status: "running"` 提示
- **agent_id 稳定性**：`sub-{thread}-{agent}-{depth}-{seq}` 含全局序号，跨进程重启后 registry（内存态）丢失，子 session 元数据以 SQLite 为准 —— P1 落库后无影响
- **acp_sessions 与 checkpoints 双源**：`session/list` 目前从 checkpoints 表聚合，`acp_sessions` 只是 ACP 元数据侧车；引入 parent_id 后子会话必须两条链路一致，建议 list 查询改走 `acp_sessions` 主表 + checkpoints 聚合 JOIN（与 `list_sessions_from_db` 的 CTE 合并）
- **OpenChamber 主仓 vs feat-dev**：`C:\Users\heycj\dev\openchamber`（main）尚未合入 `src/lib/acp/` 契约代码，本设计以 `openchamber-feat-dev` 的契约为准；main 合入 ACP 支持后需回归
