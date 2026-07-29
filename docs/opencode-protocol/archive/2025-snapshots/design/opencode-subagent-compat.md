# Loom server ↔ OpenChamber 子代理协议兼容层

> 日期: 2025-08-19
> 状态: 设计完成，待实施
> 范围: 仅 `apps/server/`，Loom core 与 OpenChamber 均零改动

## 1. 背景

**OpenChamber**（前端，Bun/TS）通过 `@opencode-ai/sdk/v2` 调用后端，对"子代理"的建模是 **child session**：

- LLM 调用 `task` 工具 → 后端创建 `Session`，`parentID` 指向当前 session
- 工具输出包含 `<task id="..." status="...">` 标签，前端用正则 `taskSessionIdParser.ts:3` 解析
- 双窗口 fallback 算法（`resolveFallbackTaskSessionId.ts`，3s/8s）按 `parentID` + `time.created` 在 `api.session.list({directory})` 结果中匹配子 session
- sidebar 通过 `useMobileSessionTreeStore` 按 `parentID` 建树
- 子 session 可独立打开、查 messages、删除

**Loom**（后端，Rust/Axum）的子代理是 **`AgentTool`**：

- 输入：`{ agent: "profile-name", task: "...", background?: bool }`（`agent/agent-core/src/tools/agent/mod.rs:140`）
- 输出：`{ agent_id: "sub-{parent_thread}-{name}-{depth}-{seq}", status: "running" }` JSON
- 独立完整 ReAct 循环，不在 `SessionInfo` 表里
- 通过 `AsyncAgentRegistry`（`agent/agent-core/src/tools/agent/registry.rs`）管理

**差距**：
- Loom 无 `Session.parentID` 自动建立（虽 `state.rs:167` 字段存在，但 AgentTool 不创建 child session）
- Loom 输出 JSON `{"agent_id":...}`，OpenChamber 期望 `<task id>` 标签
- OpenChamber 通过 `api.session.list` 看不到 Loom 的 sub-agent（它们不在 SessionInfo 表）

## 2. 设计原则

| 不动 | 改动 |
|---|---|
| Loom core（`agent/`、`foundation/`、`experimental/`） | — |
| AgentTool / AsyncAgentRegistry / ReAct / plan_bridge 内部逻辑 | — |
| OpenChamber 所有源码（含 `taskSessionIdParser.ts`、`useMobileSessionTreeStore`、`SessionSidebar`） | — |
| — | `apps/server/src/opencode_compat/` 新增 4 模块 |
| — | `apps/server/src/state.rs` 加 1 个 map 字段 |
| — | `apps/server/src/handlers/session.rs::delete_session` 加 4 行级联取消 |
| — | `apps/server/src/main.rs` 加 1 行 install |

**唯一允许动 Loom core 的地方**：如果发现 `GlobalEvent` 枚举缺字段（见 §10），那是协议层缺失，需要在 `state.rs` 加字段，但**不改 core crates 的语义**。

## 3. 架构

```
                     OpenChamber (零改动)
                           │
                           │ HTTP/SSE: /session/*, /api/session/*, /event
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                    apps/server (Loom HTTP)                  │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  handlers/session.rs (既有)                            │  │
│  │  /session, /session/:id, /session/:id/children,        │  │
│  │  /session/:id/prompt, PATCH parent_id                  │  │
│  └───────────────────────────────────────────────────────┘  │
│                            │                                │
│                            ▼                                │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  opencode_compat/  ★ 本次新增                          │  │
│  │  ├── session_bridge.rs      ToolStart → 建 child       │  │
│  │  ├── output_rewriter.rs     ToolEnd → 改 <task id>    │  │
│  │  ├── event_fanout.rs        事件 fan-out 到 child SSE │  │
│  │  └── message_aggregator.rs  子消息 → child.messages   │  │
│  └───────────────────────────────────────────────────────┘  │
│                            │                                │
│                            ▼                                │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  state.rs: state.events (GlobalEvent bus)              │  │
│  │  state.agent_call_map: HashMap<call_id, child_session> │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
         Loom core: AgentTool, AsyncAgentRegistry,
                    ReAct runner, plan_bridge (零改动)
```

## 4. 数据结构

在 `apps/server/src/state.rs::SharedState` 加 1 个字段：

```rust
/// 映射 AgentTool 的 call_id 到 Loom server 适配层生成的 child session。
/// 由 session_bridge.rs 写入，output_rewriter / event_fanout /
/// message_aggregator / delete_session 读取。
pub agent_call_map: parking_lot::Mutex<HashMap<String, AgentCallEntry>>,
```

```rust
#[derive(Clone)]
pub struct AgentCallEntry {
    pub child_session_id: String,
    pub agent_name: String,
    pub parent_session_id: Option<String>,
    pub started_at: i64,
}
```

child session 本身复用现有 `SessionInfo`（`state.rs:158`），仅在创建后用 `apply_session_patch` 设置 `parent_id`（已有逻辑，`handlers/session.rs:988`）。

## 5. 模块详细设计

### 5.1 session_bridge.rs — AgentTool ToolStart → 建 child session

**入口**：`install(state)` 在 `main.rs` 启动时调用一次。

**监听**：订阅 `state.events` 总线。

**触发**：收到 `GlobalEvent::ToolStart { name: "agent", call_id, args, parent_session_id }`。

**动作**：
1. 解析 `args.agent`（默认 `"default"`）和 `args.task`（截取前 60 字符做标题）
2. `make_session(&state, Some(agent_name))` 生成 child session
3. 设 `child.parent_id = parent_session_id`
4. 设 `child.title = format!("[{}] {}", agent_name, task_preview)`
5. `state.sessions.write().insert(child.id.clone(), child.clone())` + `persist_session`
6. `emit(&state, "session.created", json!({ "sessionID": child.id, "info": child }))`
7. `state.agent_call_map.lock().insert(call_id, AgentCallEntry { ... })`
8. `message_aggregator::spawn(state.clone(), call_id.clone(), child.id.clone())`

**关键**：动作 1-7 是**同步**的（不进 tokio::spawn 内部 await），保证 child session 在 emit `session.created` 时已经存在于 `state.sessions`。这是 OpenChamber fallback 算法能命中子 session 的前提。

### 5.2 output_rewriter.rs — AgentTool ToolEnd → 输出改写

**入口**：作为 middleware 挂在 SSE emit 路径上（在 `state.rs::emit` 内 `broadcast` 之前调用 `rewrite(&state, event)`）。

**触发**：`event.name == "agent"` 且有 `call_id`。

**动作**：
1. 从 `state.agent_call_map` 查 `child_session_id` 和 `agent_name`
2. 解析原 `result`（Loom AgentTool 的 JSON 输出）：
   - 同步模式：`{"agent_id": "sub-...", "status": "running"}` —— result 是 ToolEnd 的 assistant message 内容
   - background 模式：`{"agent_id": "...", "status": "background", "message": "..."}`
   - 失败模式：`is_error == true`
3. 改写为 OpenCode 风格：
   - 成功：`<task id="{child_id}" agent="{agent_name}" status="running">`
   - background：`<task id="{child_id}" agent="{agent_name}" status="background" message="...">`
   - 失败：`<task id="{child_id}" agent="{agent_name}" status="failed">{error_msg}</task>`
4. 返回改写后的 event

**注意**：原 `result` 中的 Loom `agent_id`（`sub-...-{seq}`）保留在 `AgentCompletionStats` 等结构里，但**不暴露**给前端。OpenChamber 只看 `<task id>` 里的 `child_session_id`（这是它能 list/messages/delete 的真实 session id）。

### 5.3 event_fanout.rs — sub-agent 事件复制到 child session SSE

**入口**：作为 broadcast 前置 hook 调用 `fanout(&state, event) -> Vec<GlobalEvent>`。

**触发**：任何带 `call_id` 的事件，且 `call_id` 在 `agent_call_map` 中。

**动作**：
1. 原 event 保留（让父 session 的订阅者收到）
2. 克隆 event，`set_session_id(child_session_id)`（替换 `sessionID` 字段）
3. 返回 `[原 event, 克隆 event]`

**关键**：替换 `sessionID` 后，前端 `api.event.subscribe({ sessionID: child_id })` 能收到完整子代理事件流。

**不 fan-out 的事件**：
- AgentCompleted / AgentFailed（这些由 message_aggregator 内部处理）
- session.created / session.deleted / session.updated（已经在原始 session 维度 emit）

### 5.4 message_aggregator.rs — 子消息聚合到 child session

**入口**：`spawn(state, call_id, child_session_id)`，每个 child session 一个后台 task。

**订阅**：`state.events.subscribe()`，过滤条件 `event.belongs_to_call(&call_id)`。

**缓冲**：维护 `Vec<MessageInfo>` buffer，按时间顺序追加。

**匹配规则**（按 event 类型）：
| Event | 动作 |
|---|---|
| `Message { role, content, ... }` | `buffer.push(MessageInfo)` |
| `ToolStart { info, ... }` | 找到最近一个 `assistant` message，push 一个 tool part |
| `ToolEnd { info, result, is_error, ... }` | 找到对应的 tool part，更新 `state` 和 `result` |
| `Plan { entries }`（来自 plan_bridge） | 找到最近一个 `assistant` message，push 一个 plan part |
| `AgentCompleted { call_id } == ours` | buffer 一次性写入 `state.messages[child_session_id]`，`persist_messages`，break |
| `AgentFailed { call_id } == ours` | 同上但最后一个 message `finish = "failed"`，break |

**结束清理**：`agent_call_map.remove(&call_id)`。

### 5.5 delete_session 扩展（既有 handler 微调）

`apps/server/src/handlers/session.rs::delete_session`（约 line 119）在 `persist_session_delete` 之后插入：

```rust
// 级联取消关联的 sub-agent
let to_cancel: Vec<String> = state.agent_call_map.lock().unwrap()
    .iter()
    .filter(|(_, entry)| entry.child_session_id == id)
    .map(|(call_id, _)| call_id.clone())
    .collect();
for call_id in to_cancel {
    state.agent_registry.cancel(&call_id);  // AsyncAgentRegistry 已有
    state.agent_call_map.lock().unwrap().remove(&call_id);
}
```

## 6. 时序

```
T0  LLM emit tool_call name="agent" call_id=c1 args={agent,task}
    │ (state.events)
    ▼
T1  session_bridge 同步执行:
    - make_session → child.id = "sess_xxx"
    - child.parent_id = current_session_id
    - emit session.created { sessionID: "sess_xxx" }    ← OpenChamber 立刻看到
    - agent_call_map[c1] = { child: "sess_xxx", agent: "..." }
    - spawn message_aggregator(c1, "sess_xxx")
    │
T2  AgentTool 内部 spawn ReAct (Loom core, 零改动)
    │
    │ 期间: ToolStart/ToolEnd/Message/Plan 事件通过 state.events emit
    │
    ├→ output_rewriter: 不匹配 (ToolStart 在 result 出来之前)
    ├→ event_fanout: 复制 + set_session_id → 父 SSE 和 child SSE 都收到
    └→ message_aggregator: 累加到 buffer
    │
T3  AgentTool 完成 (Loom core), emit ToolEnd { name: "agent", result: JSON, call_id: c1 }
    │
    ├→ output_rewriter: 解析 JSON → <task id="sess_xxx" status="running">
    │   → emit 给父 session SSE
    │
    └→ AgentCompleted { call_id: c1 } (Loom core)
        └→ message_aggregator: buffer → state.messages["sess_xxx"]
                                  → persist_messages
                                  → break
                                  → agent_call_map.remove(c1)
    │
T4  OpenChamber 收到 <task id="sess_xxx">
    - taskSessionIdParser 正则匹配
    - sidebar 树形立刻显示 sess_xxx 节点
    - status 通过 session.list + 时间窗口 fallback 消歧
    │
T5  用户点击 sess_xxx → api.session.messages({sessionID: "sess_xxx"})
    → 拿到 T3 聚合后的完整消息历史（含 sub-agent 的 tool_call、text、plan）
```

## 7. 边界条件

| 情况 | 处理 |
|---|---|
| AgentTool 同步调用（默认） | T3 输出 `status="running"`，T3' 子代理实际已完成 |
| AgentTool background=true | T3 输出 `status="background"`，T3' 完成后聚合器收尾 |
| AgentTool timeout 转 background | 输出 `status="background" message="timeout after Xs..."` |
| AgentTool 失败（is_error） | output_rewriter 输出 `status="failed"`，aggregator `finish="failed"` |
| 多个并发 AgentTool | 每个 call_id 独立映射，aggregator 各自独立 |
| 嵌套 sub-agent（sub-agent 又调 sub-agent） | 递归工作：内层 AgentTool 也会被 session_bridge 监听到 |
| AgentTool 调用时 args.worker_folder 覆盖 | session_bridge 把 `args.worker_folder` 作为 `child.directory` |
| AgentTool worktree 隔离 | Loom core 已做 git worktree 创建；child.directory 反映 worktree 路径 |

## 8. 改动清单

| 文件 | 类型 | 行数 | 说明 |
|---|---|---|---|
| `apps/server/src/opencode_compat/mod.rs` | 新增 | ~30 | `pub fn install(state)` 入口 |
| `apps/server/src/opencode_compat/session_bridge.rs` | 新增 | ~90 | 监听 ToolStart → 建 child |
| `apps/server/src/opencode_compat/output_rewriter.rs` | 新增 | ~70 | ToolEnd JSON → `<task id>` |
| `apps/server/src/opencode_compat/event_fanout.rs` | 新增 | ~50 | 事件复制 + set_session_id |
| `apps/server/src/opencode_compat/message_aggregator.rs` | 新增 | ~120 | 子消息聚合 |
| `apps/server/src/state.rs` | 修改 | +5 | `agent_call_map: Mutex<HashMap<...>>` |
| `apps/server/src/handlers/session.rs` | 修改 | +8 | `delete_session` 加级联取消 |
| `apps/server/src/main.rs` | 修改 | +1 | `opencode_compat::install(state.clone())` |
| `apps/server/src/state.rs::emit` | 修改 | +3 | 在 broadcast 前调 `output_rewriter::rewrite` + `event_fanout::fanout` |
| `apps/server/tests/opencode_compat_test.rs` | 新增 | ~200 | 9 个 e2e 测试 |
| **合计** | | **~580 行** | |

**Loom 其他 crate（`agent/`、`foundation/`、`experimental/`）**：0 改动
**OpenChamber**：0 改动

## 9. 验证矩阵

| 测试 | 验证点 | 文件 |
|---|---|---|
| `session_bridge_creates_child_on_agent_tool_start` | ToolStart 后 child session 出现在 `state.sessions` | `tests/opencode_compat_test.rs` |
| `child_has_correct_parent_id` | `child.parentID == parent.session_id` | 同上 |
| `child_emits_session_created_event` | `emit("session.created")` 被调用，含 child info | 同上 |
| `output_rewriter_produces_task_tag_running` | ToolEnd 输出含 `<task id="..." status="running">` | 同上 |
| `output_rewriter_handles_background_status` | 输出 `<task id="..." status="background" message="...">` | 同上 |
| `output_rewriter_handles_error` | 输出 `<task id="..." status="failed">` | 同上 |
| `event_fanout_replicates_to_child_stream` | 子 tool_call 同时出现在父和 child 的 SSE 流 | 同上 |
| `aggregator_writes_messages_to_child` | `state.messages[child_id]` 含完整 sub-agent 对话 | 同上 |
| `aggregator_marks_finish_failed_on_error` | 失败时最后一个 message `finish="failed"` | 同上 |
| `delete_session_cancels_subagent` | DELETE child → `AsyncAgentRegistry::cancel` 被调 | 同上 |
| `concurrent_subagents_independent` | 两个并发 AgentTool，call_id 不同，child 独立 | 同上 |
| `nested_subagent_creates_grandchild` | 嵌套调用也建 grandchild session | 同上 |

## 10. 待协调点（实施前确认）

这些是 Loom 现有 API 表面（不是新设计），需要在写代码前查清：

1. **`GlobalEvent` 枚举字段完整性**
   - 当前 `state.rs::emit` 调用点列表是否齐全？需要看到所有 `emit(&state, "...", json!({...}))` 调用，确认 `ToolStart`、`ToolEnd`、`Message`、`AgentCompleted`、`AgentFailed`、`Plan` 都已存在
   - `ToolStart` / `ToolEnd` 是否带 `call_id` 和 `parent_session_id` 字段？
   - `Message` 事件是否已 emit，还是只 emit 到 SSE 流？
   - 位置：`apps/server/src/state.rs`（grep `pub enum GlobalEvent` 和所有 `emit(` 调用）

2. **`AsyncAgentRegistry::cancel` 方法**
   - `agent/agent-core/src/tools/agent/registry.rs` 是否有 `pub fn cancel(&self, call_id: &str)`？
   - 如果只有 `AbortHandle`，需要从 `agent_call_map` 存 `JoinHandle::abort_handle()`

3. **`SessionInfo.directory` 来源**
   - `apply_session_directory_override`（`handlers/session.rs:994`）现有逻辑：是否支持从 `args.worker_folder` 写入？
   - AgentTool 调 `args.worker_folder` 时，如何传递到 child session 的 `directory` 字段？

4. **`plan_bridge` 事件 emit**
   - `apps/acp/tests/plan_bridge_test.rs` 测试的是 `loom_event_to_updates`（内部函数），不直接 emit GlobalEvent
   - server 层是否已在 SSE 流中 emit `Plan` 事件？还是只在 ACP 协议层 emit？
   - 如果只在 ACP 层，server 需要新加 `emit(&state, "plan", ...)` 调用 —— 这是 §8 的 +3 行之一

5. **OpenChamber 实际请求格式**
   - OpenChamber 的 `api.session.list` 调的是 `directory` 还是 `directoryID`？（看 `useAgentGroupsStore.ts:177`）
   - child session 创建后需要 set `directory` 字段，否则 OpenChamber 拉不到

## 11. 风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| ToolStart 事件未被 server emit（只在 ACP 层） | session_bridge 监听不到 | 在 server SSE 流中补 emit `ToolStart`/`ToolEnd` —— §8 改动覆盖 |
| AgentTool 内部用 tokio::spawn，call_id 是内部 UUID | call_id 在 server 层不可见 | session_bridge 改监听 ToolEnd 反查 args.parent_session_id，或扩展 AgentTool 公开 call_id（**仅改 state.rs 字段**，不动 core 逻辑） |
| child session 与 Loom 内部 sub-agent 状态不一致 | sidebar 显示 running，实际已完成 | aggregator 在 AgentCompleted 时同步更新 child 的 `time.updated` |
| OpenChamber 调 `api.session.delete({ sessionID: child })` 时聚合未完成 | 部分消息丢失 | aggregator 在 cancel 时把 buffer 也写入（即使 finish="cancelled"） |
| 持久化层（SQLite/JSON）需支持 parent_id 字段 | 老数据迁移 | `apps/cli/src/session.rs:682` schema 已含 `parent_session_id TEXT` 列，OK |

## 12. 实施顺序

1. **盘点阶段**：先读 §10 的 5 个点，确认现有 API 表面，**不要先写代码**
2. **第 1 步**：实现 `session_bridge.rs` + 单测（最关键，决定整套机制能否工作）
3. **第 2 步**：实现 `output_rewriter.rs` + 单测
4. **第 3 步**：实现 `message_aggregator.rs` + 单测
5. **第 4 步**：实现 `event_fanout.rs` + 单测
6. **第 5 步**：改 `delete_session` + 集成测试
7. **第 6 步**：端到端联调（手动 mock LLM 触发 AgentTool → curl 验证）
8. **第 7 步**：OpenChamber 联调（开 sidebar 看 child 出现 + fallback 工作）