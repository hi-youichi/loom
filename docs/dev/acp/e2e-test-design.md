# loom-acp E2E Test Design

> 基于 [Agent Client Protocol](https://agentclientprotocol.com/protocol/overview) 规范，对 loom-acp 进行端到端测试设计。
> E2E 测试通过子进程启动 `loom-acp` 二进制，经由 stdin/stdout JSON-RPC 通信，验证完整的协议生命周期。

## 测试架构

```
┌─────────────┐    stdin (JSON-RPC)    ┌─────────────┐
│  Test Runner │ ──────────────────────►│  loom-acp   │
│  (parent)    │ ◄──────────────────────│  (child)    │
└─────────────┘    stdout (JSON-RPC)    └─────────────┘
```

### 公共基础设施 (`tests/e2e/mod.rs`)

```rust
struct AcpChild {
    child: Child,
    next_id: u64,
}

impl AcpChild {
    /// 启动 loom-acp 子进程，带可选 --log-file
    fn spawn(log_file: Option<&Path>) -> Self;

    /// 发送 JSON-RPC request，返回 id
    fn send_request(&mut self, method: &str, params: serde_json::Value) -> u64;

    /// 发送 JSON-RPC notification（无 id）
    fn send_notification(&mut self, method: &str, params: serde_json::Value);

    /// 读取下一行 JSON-RPC 消息（带超时）
    fn read_message(&mut self) -> Result<serde_json::Value>;

    /// 读取所有消息直到收到指定 id 的 response
    fn read_response(&mut self, id: u64) -> Result<serde_json::Value>;

    /// 读取所有消息直到收到指定 method 的 notification
    fn read_notification(&mut self, method: &str) -> Result<serde_json::Value>;

    /// 读取 response 并反序列化为具体类型
    fn read_response_as<T: DeserializeOwned>(&mut self, id: u64) -> Result<T>;

    /// 完整初始化流程: initialize → (可选 authenticate) → session/new
    fn full_handshake(&mut self) -> String; // returns session_id

    /// 关闭子进程
    fn shutdown(&mut self);
}
```

---

## 测试用例

### Phase 1: Initialization — 连接建立

| # | 测试 | ACP 方法 | 验证点 |
|---|------|----------|--------|
| 1.1 | `e2e_initialize_returns_capabilities` | `initialize` | 返回 `protocolVersion: 1`，`agentCapabilities` 包含 `loadSession`, `listTools`, `promptCapabilities`，`agentInfo` 包含 name/version/title |
| 1.2 | `e2e_initialize_rejects_unsupported_version` | `initialize` (version=999) | 返回 JSON-RPC error，code = `-32600` (Invalid Request) 或包含 version mismatch 信息 |
| 1.3 | `e2e_authenticate_succeeds` | `authenticate` | 无 auth 配置时返回成功响应 |

### Phase 2: Session Lifecycle — 会话管理

| # | 测试 | ACP 方法 | 验证点 |
|---|------|----------|--------|
| 2.1 | `e2e_new_session_returns_session_id` | `session/new` | 返回非空 `sessionId` 字符串 |
| 2.2 | `e2e_new_session_includes_modes` | `session/new` | 响应包含 `modes.availableModes` 列表，至少有一个 mode（如 "code"） |
| 2.3 | `e2e_new_session_includes_config_options` | `session/new` | 响应包含 `configOptions`，其中 model 选项列出可用模型 |
| 2.4 | `e2e_load_session_recovers_state` | `session/load` | 先 `session/new` → 发 prompt → `session/load` 用同一 sessionId → 历史消息恢复 |
| 2.5 | `e2e_load_session_unknown_id_returns_error` | `session/load` | 用不存在的 sessionId 返回错误 |
| 2.6 | `e2e_multiple_sessions_independent` | `session/new` × 2 | 创建两个 session，各自 prompt 互不干扰 |

### Phase 3: Prompt Turn — 对话交互

| # | 测试 | ACP 方法 | 验证点 |
|---|------|----------|--------|
| 3.1 | `e2e_prompt_simple_text_response` | `session/prompt` | 发送简单 prompt（如 "say hello"），收到 `session/update` notifications，最终 `stopReason: "endTurn"` |
| 3.2 | `e2e_prompt_sends_content_notifications` | `session/prompt` | 验证 notification 中包含 `contentBlock` 类型为 `text`，且 `content` 非空 |
| 3.3 | `e2e_prompt_tool_call_flow` | `session/prompt` | prompt 触发工具调用 → 收到 `toolCall` notification → 客户端 `request_permission` → agent 继续 → `endTurn` |
| 3.4 | `e2e_prompt_cancel_stops_execution` | `session/prompt` + `cancel` | prompt 开始后发送 cancel notification，agent 停止并发送 `stopReason: "cancelled"` |
| 3.5 | `e2e_prompt_empty_text_returns_error` | `session/prompt` | 空 prompt 或无效内容返回错误 |

### Phase 4: Tools — 工具系统

| # | 测试 | ACP 方法 | 验证点 |
|---|------|----------|--------|
| 4.1 | `e2e_list_tools_returns_available_tools` | `listTools` | 返回工具列表，包含 `read_text_file`, `write_text_file`, `create_terminal` 等工具名 |
| 4.2 | `e2e_list_tools_includes_input_schema` | `listTools` | 每个工具包含 `inputSchema` (JSON Schema)，描述参数结构 |
| 4.3 | `e2e_tool_read_file_success` | tool call via prompt | prompt 要求读取文件 → tool call → permission granted → 读取成功 → agent 获得文件内容 |
| 4.4 | `e2e_tool_read_file_not_found` | tool call via prompt | prompt 要求读取不存在的文件 → tool call → 返回错误信息 |
| 4.5 | `e2e_tool_write_creates_file` | tool call via prompt | prompt 要求创建文件 → tool call → permission granted → 文件存在于磁盘 |

### Phase 5: Session Configuration — 会话配置

| # | 测试 | ACP 方法 | 验证点 |
|---|------|----------|--------|
| 5.1 | `e2e_set_session_config_model` | `session/setConfigOption` | 设置 model config option → 新 prompt 使用指定模型 |
| 5.2 | `e2e_set_session_config_invalid_option` | `session/setConfigOption` | 设置不存在的 config option id → 返回错误 |
| 5.3 | `e2e_set_session_mode` | `session/setMode` | 设置 mode（如 "code"）→ 响应确认 → 后续 prompt 使用该 mode |
| 5.4 | `e2e_set_session_mode_invalid` | `session/setMode` | 设置不存在的 mode → 返回错误 |

### Phase 6: Error Handling — 错误处理

| # | 测试 | ACP 方法 | 验证点 |
|---|------|----------|--------|
| 6.1 | `e2e_invalid_json_returns_parse_error` | raw invalid JSON | 发送非法 JSON → 返回 code `-32700` (Parse error) |
| 6.2 | `e2e_unknown_method_returns_method_not_found` | `unknown/method` | 发送不存在的方法 → 返回 code `-32601` (Method not found) |
| 6.3 | `e2e_prompt_without_session_returns_error` | `session/prompt` (no session) | 未创建 session 直接发 prompt → 返回错误 |
| 6.4 | `e2e_concurrent_prompt_returns_error` | `session/prompt` × 2 并发 | 正在执行 prompt 时再发一个 → 返回 busy/冲突错误 |

### Phase 7: Transport & Robustness — 传输与健壮性

| # | 测试 | ACP 方法 | 验证点 |
|---|------|----------|--------|
| 7.1 | `e2e_process_exits_on_stdin_close` | close stdin | 关闭 stdin → loom-acp 进程正常退出（exit code 0） |
| 7.2 | `e2e_log_file_created` | `--log-file` | 启动时指定 `--log-file` → 日志文件存在于磁盘 |
| 7.3 | `e2e_multiple_initialize_handled` | `initialize` × 2 | 重复 initialize → 不崩溃，返回合理的响应或错误 |

---

## 实现优先级

### P0 — 必须实现（覆盖核心生命周期）

1. **1.1** `e2e_initialize_returns_capabilities`
2. **1.3** `e2e_authenticate_succeeds`
3. **2.1** `e2e_new_session_returns_session_id`
4. **3.1** `e2e_prompt_simple_text_response`
5. **6.1** `e2e_invalid_json_returns_parse_error`
6. **7.1** `e2e_process_exits_on_stdin_close`

### P1 — 重要（覆盖主要协议功能）

7. **2.2** `e2e_new_session_includes_modes`
8. **2.3** `e2e_new_session_includes_config_options`
9. **3.2** `e2e_prompt_sends_content_notifications`
10. **4.1** `e2e_list_tools_returns_available_tools`
11. **4.2** `e2e_list_tools_includes_input_schema`
12. **6.2** `e2e_unknown_method_returns_method_not_found`

### P2 — 完善（覆盖边界和高级场景）

13. **2.4** `e2e_load_session_recovers_state`
14. **2.5** `e2e_load_session_unknown_id_returns_error`
15. **2.6** `e2e_multiple_sessions_independent`
16. **3.3** `e2e_prompt_tool_call_flow`
17. **3.4** `e2e_prompt_cancel_stops_execution`
18. **4.3-4.5** tool 交互测试
19. **5.1-5.4** session 配置测试
20. **7.2-7.3** 健壮性测试

---

## 注意事项

- **需要 LLM 后端**: prompt 相关测试（3.x, 4.3-4.5, 5.1, 5.3）依赖真实 LLM API，应标记为 `#[ignore]` 或通过 feature gate 控制
- **工具测试需要文件系统**: 4.3-4.5 使用 tempdir 隔离
- **超时设置**: 每个测试设置合理的 read 超时（如 30s），prompt 测试可适当放宽
- **日志调试**: 所有测试启用 `--log-file`，失败时打印日志内容辅助调试
- **并行安全**: 多个测试同时运行时，各自使用独立的 tempdir 和 log file
