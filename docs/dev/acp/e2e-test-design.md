# loom-acp E2E Test Design

基于 [Agent Client Protocol](https://agentclientprotocol.com) 规范，对 loom-acp 进行端到端测试设计。
E2E 测试通过子进程启动 `loom-acp` 二进制，经由 stdin/stdout JSON-RPC 通信，验证完整的协议生命周期。


## 测试用例

基于现有实现，测试用例已按功能模块组织在以下文件中：

### Phase 1: Initialization — 连接建立

| # | 测试 | ACP 方法 | 验证点 | 状态 | 文件 |
|---|------|----------|--------|------|------|
| 1.1 | `e2e_initialize_returns_capabilities` | `initialize` | 返回 `protocolVersion: 1`，`agentCapabilities` 包含 `loadSession`, `listTools`, `promptCapabilities`，`agentInfo` 包含 name/version/title | 已实现 | `initialization_detailed.rs` |
| 1.2 | `e2e_initialize_rejects_unsupported_version` | `initialize` (version=999) | 返回 JSON-RPC error，code = `-32600` (Invalid Request) 或包含 version mismatch 信息 | 已实现 | `initialization_detailed.rs` |
| 1.3 | `e2e_authenticate_succeeds` | `authenticate` | 无 auth 配置时返回成功响应 | 已实现 | `initialization_detailed.rs` |

### Phase 2: Session Lifecycle — 会话管理

| # | 测试 | ACP 方法 | 验证点 | 状态 | 文件 |
|---|------|----------|--------|------|------|
| 2.1 | `e2e_new_session_returns_session_id` | `session/new` | 返回非空 `sessionId` 字符串 | 已实现 | `session_lifecycle.rs` |
| 2.2 | `e2e_new_session_includes_modes` | `session/new` | 响应包含 `modes.availableModes` 列表，至少有一个 mode（如 "code"） | 已实现 | `session_lifecycle.rs` |
| 2.3 | `e2e_new_session_includes_config_options` | `session/new` | 响应包含 `configOptions`，其中 model 选项列出可用模型 | 已实现 | `session_lifecycle.rs` |
| 2.4 | `e2e_load_session_recovers_state` | `session/load` | 先 `session/new` → 发 prompt → `session/load` 用同一 sessionId → 历史消息恢复 | 未实现 | - |
| 2.5 | `e2e_load_session_unknown_id_returns_error` | `session/load` | 用不存在的 sessionId 返回错误 | 未实现 | - |
| 2.6 | `e2e_multiple_sessions_independent` | `session/new` × 2 | 创建两个 session，各自 prompt 互不干扰 | 未实现 | - |

### Phase 3: Prompt Turn — 对话交互

| # | 测试 | ACP 方法 | 验证点 | 状态 | 文件 |
|---|------|----------|--------|------|------|
| 3.1 | `e2e_prompt_simple_text_response` | `session/prompt` | 发送简单 prompt（如 "say hello"），收到 `session/update` notifications，最终 `stopReason: "endTurn"` | 未实现 | - |
| 3.2 | `e2e_prompt_sends_content_notifications` | `session/prompt` | 验证 notification 中包含 `contentBlock` 类型为 `text`，且 `content` 非空 | 未实现 | - |
| 3.3 | `e2e_prompt_tool_call_flow` | `session/prompt` | prompt 触发工具调用 → 收到 `toolCall` notification → 客户端 `request_permission` → agent 继续 → `endTurn` | 未实现 | - |
| 3.4 | `e2e_prompt_cancel_stops_execution` | `session/prompt` + `cancel` | prompt 开始后发送 cancel notification，agent 停止并发送 `stopReason: "cancelled"` | 未实现 | - |
| 3.5 | `e2e_prompt_empty_text_returns_error` | `session/prompt` | 空 prompt 或无效内容返回错误 | 未实现 | - |

### Phase 4: Tools — 工具系统

| # | 测试 | ACP 方法 | 验证点 | 状态 | 文件 |
|---|------|----------|--------|------|------|
| 4.1 | `e2e_list_tools_returns_available_tools` | `listTools` | 返回工具列表，包含 `read_text_file`, `write_text_file`, `create_terminal` 等工具名 | 未实现 | - |
| 4.2 | `e2e_list_tools_includes_input_schema` | `listTools` | 每个工具包含 `inputSchema` (JSON Schema)，描述参数结构 | 未实现 | - |
| 4.3 | `e2e_tool_read_file_success` | tool call via prompt | prompt 要求读取文件 → tool call → permission granted → 读取成功 → agent 获得文件内容 | 未实现 | - |
| 4.4 | `e2e_tool_read_file_not_found` | tool call via prompt | prompt 要求读取不存在的文件 → tool call → 返回错误信息 | 未实现 | - |
| 4.5 | `e2e_tool_write_creates_file` | tool call via prompt | prompt 要求创建文件 → tool call → permission granted → 文件存在于磁盘 | 未实现 | - |

### Phase 5: Session Configuration — 会话配置

| # | 测试 | ACP 方法 | 验证点 | 状态 | 文件 |
|---|------|----------|--------|------|------|
| 5.1 | `e2e_set_session_config_model` | `session/setConfigOption` | 设置 model config option → 新 prompt 使用指定模型 | 未实现 | - |
| 5.2 | `e2e_set_session_config_invalid_option` | `session/setConfigOption` | 设置不存在的 config option id → 返回错误 | 未实现 | - |
| 5.3 | `e2e_set_session_mode` | `session/setMode` | 设置 mode（如 "code"）→ 响应确认 → 后续 prompt 使用该 mode | 未实现 | - |
| 5.4 | `e2e_set_session_mode_invalid` | `session/setMode` | 设置不存在的 mode → 返回错误 | 未实现 | - |

### Phase 6: Error Handling — 错误处理

| # | 测试 | ACP 方法 | 验证点 | 状态 | 文件 |
|---|------|----------|--------|------|------|
| 6.1 | `e2e_invalid_json_returns_parse_error` | raw invalid JSON | 发送非法 JSON → 返回 code `-32700` (Parse error) | 已实现 | `initialization.rs` |
| 6.2 | `e2e_unknown_method_returns_method_not_found` | `unknown/method` | 发送不存在的方法 → 返回 code `-32601` (Method not found) | 未实现 | - |
| 6.3 | `e2e_prompt_without_session_returns_error` | `session/prompt` (no session) | 未创建 session 直接发 prompt → 返回错误 | 未实现 | - |
| 6.4 | `e2e_concurrent_prompt_returns_error` | `session/prompt` × 2 并发 | 正在执行 prompt 时再发一个 → 返回 busy/冲突错误 | 未实现 | - |

### Phase 7: Transport & Robustness — 传输与健壮性

| # | 测试 | ACP 方法 | 验证点 | 状态 | 文件 |
|---|------|----------|--------|------|------|
| 7.1 | `e2e_process_exits_on_stdin_close` | close stdin | 关闭 stdin → loom-acp 进程正常退出（exit code 0） | 已实现 | `initialization.rs` |
| 7.2 | `e2e_log_file_created` | `--log-file` | 启动时指定 `--log-file` → 日志文件存在于磁盘 | 未实现 | - |
| 7.3 | `e2e_multiple_initialize_handled` | `initialize` × 2 | 重复 initialize → 不崩溃，返回合理的响应或错误 | 未实现 | - |

### 其他测试文件

除了上述核心测试，还有以下测试文件：

| 文件 | 测试内容 |
|------|----------|
| `agent_integration.rs` | 代理集成测试 |
| `agent_modes.rs` | 代理模式测试 |
| `log_file_subprocess.rs` | 日志文件子进程测试 |
| `test_content_types.rs` | 内容类型测试 |
| `test_location.rs` | 位置测试 |
| `test_terminal_integration.rs` | 终端集成测试 |
