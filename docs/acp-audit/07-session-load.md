# ACP 协议审计：session/load

## 协议规范

`session/load` 是 Client → Agent 请求，用于加载现有 session 并回放其对话历史。根据 ACP 规范（`apps/acp/src/protocol.rs:81-84`）：

- 仅在 `capabilities.loadSession: true` 时声明
- 请求字段：`session_id`、`working_directory`、`mcp_servers`
- Agent 使用 `session_id` 作为 `thread_id` 从存储加载消息/状态
- Agent 通过 `session/update` 发送 `user_message_chunk` / `agent_message_chunk` 通知回放历史
- Agent 连接请求中的 MCP 服务器
- Agent 返回 `LoadSessionResponse`

## 实现状态

**已实现** — Loom 完整实现了 `session/load` 协议规范。

## 实现细节

### 能力注册

**文件：** `apps/acp/src/agent.rs:373-395`

Loom 在初始化期间在 `agentCapabilities` 中注册 `loadSession: true`：

```rust
// Add loadSession capability by serializing, modifying, and deserializing
let mut json = serde_json::to_value(&base_response)...
if let Some(obj) = json.as_object_mut() {
    obj.insert(
        "agentCapabilities".to_string(),
        serde_json::json!({
            "loadSession": true,
            "sessionCapabilities": { "list": {}, "fork": {} },
            "promptCapabilities": { "embeddedContext": true, "image": true, "audio": true }
        }),
    );
}
```

### 请求路由

**文件：** `apps/acp/src/stdio_loop.rs:358-370`

`LoadSessionRequest` 被路由到 `agent.load_session()`：

```rust
.on_receive_request(
    move |req: LoadSessionRequest,
          responder: Responder<LoadSessionResponse>,
          _conn: ConnectionTo<Client>| {
        let agent = a_load.clone();
        async move {
            let result = agent.load_session(req).await;
            let _ = responder.respond_with_result(result);
            Ok(())
        }
    },
    on_receive_request!(),
)
```

### Session 存储：`create_with_id`

**文件：** `apps/acp/src/session.rs:139-161`

使用特定 ID 创建 session 条目（供 `session/load` 使用）：

```rust
pub fn create_with_id(
    &self,
    session_id: SessionId,
    working_directory: Option<PathBuf>,
    thread_id: String,
) -> SessionEntry {
    let mut guard = recover_write(&self.inner);
    if let Some(existing) = guard.get(&session_id) {
        return existing.clone();
    }
    let entry = SessionEntry { thread_id, working_directory, cancelled: ..., session_config: ..., cancellation: ..., mcp_servers: Vec::new() };
    guard.insert(session_id.clone(), entry.clone());
    entry
}
```

### 核心 `load_session` 实现

**文件：** `apps/acp/src/agent.rs:1048-1267`

`load_session` 方法：

1. **Session 条目管理（第 1055-1091 行）：** 通过 `create_with_id` 复用现有或创建新的 `SessionEntry`，使用 `session_id` 作为 `thread_id`。如果 `current_agent` 为空，则默认为 registry 的默认模式。

2. **检查点加载（第 1093-1189 行）：** 使用 `thread_id` 创建 `SqliteSaver` checkpointer，查询 `checkpoint.get_tuple(&config)`：
   - 如果检查点存在：提取 `ReActState.channel_values`（消息），对 user/assistant/tool/system 消息进行计数，通过 `session_update_tx` 经 `SessionNotifier::send_history()` 发送历史。
   - 如果没有检查点：记录 "No checkpoint found, starting fresh"。
   - 出错时：记录警告并继续。

3. **MCP 服务器处理（第 1192-1201 行）：** 通过 `acp_mcp_to_loom()` 将 ACP MCP 服务器转换为 Loom 格式，更新 session 的 MCP 服务器。

4. **响应构建（第 1203-1267 行）：** 从存储加载持久化配置（`mode`、`model`、`effort`），使用可用的 modes/models 构建 `config_options`，返回带有 `configOptions`、`modes` 和 `meta` 的 `LoadSessionResponse`。

## 实现方式

- **历史回放：** 使用 LangChain-rs `SqliteSaver` checkpointer 加载检查点状态。历史通过 `session_update_tx`（`notifier.send_history(&state.messages)`）作为批量发送，而非单个 chunk 消息 — 与协议意图一致但使用单次批量发送。
- **Session 标识：** `session_id` 同时作为检查点存储/检索的 `thread_id`。
- **配置持久化：** Mode、model 和 effort 从持久化配置存储加载并与内存中的 session 配置合并。
- **优雅降级：** 如果检查点不存在，方法正常进行并返回新的 session 响应。历史是可选的。

## 差距与问题

- **Chunk 粒度：** 规范提到单独发送 `user_message_chunk` / `agent_message_chunk`。实现将历史作为单次批量调用 `send_history()` 发送。这在功能上等效，但对于流式/实时客户端来说粒度较低。
- **不支持 `resume_from_node_id`：** `RunnableConfig` 使用 `resume_from_node_id: None` 构建。Loom 不支持从特定检查点节点恢复。
- **内存中 session 复用：** 通过 `new_session` 创建然后通过具有相同 `session_id` 的 `session/load` 重新加载的 session 按原样复用（第 1059 行）。这是正确行为，但值得一提。

## 验证

**对抗性验证确认了以下文件和行范围：**

| 文件 | 行 | 已验证 |
|------|-------|----------|
| `apps/acp/src/agent.rs` | 373–395 | initialize 中的 `loadSession: true` |
| `apps/acp/src/agent.rs` | 1048–1267 | 完整 `load_session` 实现 |
| `apps/acp/src/session.rs` | 139–161 | 用于 session 创建的 `create_with_id` |
| `apps/acp/src/protocol.rs` | 81–84 | 协议规范注释 |
| `apps/acp/src/stdio_loop.rs` | 358–370 | 请求路由 |
| `apps/acp/tests/agent_modes.rs` | 116–210 | 模式保留的集成测试 |

**测试覆盖包括：**
- `test_load_session_preserves_set_session_mode` — 加载前设置的 mode 被保留
- `test_load_session_new_entry_defaults_to_dev` — 新条目默认为 `dev` 模式
- `test_load_session_modes_list_contains_builtins` — 响应包括 `ask` 和 `dev`

**结论：** `完整实现 — 所有声明已验证`

## 总结

`session/load` 已完整实现并通过验证。Loom 正确声明能力、创建/检索 session 条目、从 SQLite 加载检查点历史、通过 `session/update` 向 client 回放消息、连接 MCP 服务器，并返回带有配置选项和 modes 的完整 `LoadSessionResponse`。

无严重差距。细微注释：chunk 级别的历史流被批量处理而非按消息处理，并且不支持 `resume_from_node_id` — 两者对于核心用例都不是阻塞性问题。
