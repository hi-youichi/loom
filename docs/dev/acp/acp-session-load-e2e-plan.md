# ACP session/load E2E 测试方案

## 背景

`session/load` 是 ACP 协议中恢复历史会话的核心方法。当前测试覆盖仅限于边界场景（不存在的 session、未 initialize 即 load），缺少**完整生命周期**的 e2e 验证。

**现有覆盖：**

| 文件 | 场景 |
|------|------|
| `session_capabilities_e2e.rs` | 加载不存在的 sessionId |
| `initialization_state_machine.rs` | initialize 前调用 session/load |
| `agent_modes.rs` | 单元级直接调用 `LoomAcpAgent`，验证 mode 持久化 |

**缺失：** 跨进程 checkpoint 恢复、历史通知回放、配置持久化、load 后继续对话。

## 测试架构

### 测试方式

E2E 测试通过子进程启动 `loom-acp` 二进制，经由 stdin/stdout JSON-RPC 通信。

```
┌─────────────────┐    stdin/stdout JSON-RPC    ┌─────────────────┐
│   Test Process  │ ◄──────────────────────────► │   loom-acp      │
│   (AcpChild)    │                              │   subprocess    │
└─────────────────┘                              └────────┬────────┘
                                                          │
                                                 ┌────────▼────────┐
                                                 │ Mock LLM Server │
                                                 │ (wiremock)      │
                                                 └─────────────────┘
```

### 关键基础设施

- **`AcpChild::spawn_with_mock()`** — 启动 loom-acp + wiremock LLM server
- **`send_request_and_wait(method, params, timeout)`** — 发送 JSON-RPC 请求并等待匹配 response
- **`collect_all_notifications_handling_terminal(req_id, timeout)`** — 收集 response 前的所有 `session/update` 通知
- **共享 temp home** — 两个 `AcpChild` 进程使用同一 temp 目录，确保 SQLite checkpoint 路径一致

### 新增 Helper 方法

在 `common/acp_child.rs` 中添加：

```rust
impl AcpChild {
    pub async fn load_session(
        &mut self,
        session_id: &str,
        cwd: &str,
    ) -> Result<RpcResponse, Box<dyn std::error::Error>> {
        self.send_request_and_wait(
            "session/load",
            serde_json::json!({
                "sessionId": session_id,
                "cwd": cwd,
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
    }
}
```

## 测试用例

### P0 — 核心路径

#### 1. 历史回放通知

**`e2e_load_session_replays_user_and_agent_messages`**

```
流程：initialize → session/new → prompt("Hello") → 等完成 → load(same sessionId)
验证：session/update 通知中包含 UserMessageChunk + AgentMessageChunk
```

关键点：
- prompt 完成后，SQLite checkpoint 已写入
- load 时 `SessionNotifier::send_history` 会发送历史通知
- 需用 `collect_all_notifications_handling_terminal` 捕获通知

#### 2. 跨进程 checkpoint 恢复

**`e2e_load_session_after_process_restart_restores_history`**

```
流程：
  进程 A：spawn_with_mock → initialize → session/new → prompt("Remember: X=42") → 完成 → kill
  进程 B：spawn_with_mock (同 temp home) → initialize → load(same sessionId)
验证：session/update 通知中包含历史消息（包含 "X=42"）
```

关键点：
- 两个 `AcpChild` 共享同一 temp home 目录
- `default_memory_db_path()` 解析到同一 SQLite 文件
- 这是 `session/load` 最重要的 e2e 场景

#### 3. load 后继续对话

**`e2e_prompt_after_load_session_succeeds`**

```
流程：initialize → session/new → prompt → load(same sessionId) → prompt("Follow up")
验证：第二次 prompt 成功，stopReason = "endTurn"
```

关键点：
- 验证 load 创建的 session entry 可用于后续 prompt
- 验证 thread_id 一致性

### P1 — 配置持久化

#### 4. Model 配置保留

**`e2e_load_session_preserves_model_config`**

```
流程：initialize → session/new → set_config_option(model) → load(same sessionId)
验证：response.configOptions 中包含设置的 model 值
```

#### 5. Mode 配置保留

**`e2e_load_session_preserves_mode_config`**

```
流程：initialize → session/new → set_session_mode("ask") → load(same sessionId)
验证：response.modes.currentModeId = "ask"
```

#### 6. 幂等性

**`e2e_load_session_idempotent`**

```
流程：initialize → session/new → load(sessionId) → load(sessionId)
验证：两次 load 返回相同的 modes 和 configOptions
```

#### 7. 内存中 session 复用

**`e2e_load_existing_in_memory_session_reuses_entry`**

```
流程：initialize → session/new → load(same sessionId, 不 kill 进程)
验证：load 成功，session entry 被复用（日志中 "Reusing existing session entry"）
```

### P2 — 边界与错误

#### 8. 空 sessionId

**`e2e_load_session_empty_session_id_fails`**

```
流程：initialize → load(sessionId="")
验证：返回 error
```

#### 9. 缺少 cwd

**`e2e_load_session_missing_cwd_defaults_gracefully`**

```
流程：initialize → load(sessionId=xxx, 无 cwd 字段)
验证：不崩溃，返回 result 或 error
```

#### 10. 带 mcpServers

**`e2e_load_session_with_mcp_servers_succeeds`**

```
流程：initialize → session/new → load(sessionId, mcpServers=[...])
验证：请求成功（当前 mcpServers 只记录日志）
```

#### 11. 工具调用历史回放

**`e2e_load_session_replays_tool_call_history`**

```
流程：initialize → session/new → prompt(触发 bash tool call) → load
验证：历史通知中包含 ToolCall 类型的 update
```

需要 mock 返回带 tool_calls 的 LLM 响应（参考 `mount_bash_tool_call`）。

## 测试文件组织

```
loom-acp/tests/
├── session_load_e2e.rs          # 新增：核心 P0 + P1 测试
├── session_capabilities_e2e.rs  # 现有：保留边界测试
├── initialization_state_machine.rs  # 现有：保留状态机测试
└── common/
    └── acp_child.rs             # 新增 load_session helper
```

## 实现步骤

### Step 1: 基础设施

1. 在 `AcpChild` 添加 `load_session` helper
2. 在 `AcpChild` 添加 `spawn_with_shared_home` 方法，支持传入共享 temp 目录路径

### Step 2: P0 测试

按优先级实现：
1. `e2e_load_session_replays_user_and_agent_messages`
2. `e2e_load_session_after_process_restart_restores_history`
3. `e2e_prompt_after_load_session_succeeds`

### Step 3: P1 测试

4. `e2e_load_session_preserves_model_config`
5. `e2e_load_session_preserves_mode_config`
6. `e2e_load_session_idempotent`
7. `e2e_load_existing_in_memory_session_reuses_entry`

### Step 4: P2 测试

8. `e2e_load_session_empty_session_id_fails`
9. `e2e_load_session_missing_cwd_defaults_gracefully`
10. `e2e_load_session_with_mcp_servers_succeeds`
11. `e2e_load_session_replays_tool_call_history`

## 风险与注意事项

### SQLite Checkpoint 路径

`default_memory_db_path()` 依赖 `LOOM_HOME` 环境变量。跨进程测试需确保：
- 两个进程设置相同的 `LOOM_HOME`
- 路径解析在 macOS/Linux 上行为一致

### Mock LLM 延迟

prompt 完成后才写入 checkpoint。测试需等待 prompt response 返回后再进行 load，避免竞态。

### 通知收集时序

`session/load` 的历史通知在 response **之前**发送（通过 `try_send`）。测试需要用 `collect_all_notifications` 而非 `send_request_and_wait` 来同时收集通知和 response。

### 进程清理

`AcpChild::drop` 会 kill 子进程。跨进程测试中进程 A 必须在进程 B 启动前完全退出，否则可能持有 SQLite 锁。
