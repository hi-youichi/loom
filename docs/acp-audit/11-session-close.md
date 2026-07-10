# ACP 协议审计：session/close

## 协议规范

`session/close` 协议定义为 **Client → Agent Request**，指示 agent 关闭活动 session。根据 ACP 规范，此协议使 client 能够显式终止正在进行的 session，释放关联资源并清理连接状态。

## 实现状态

**未实现**

`session/close` 协议已在 ACP 规范中记录，但在代码库中零存在。

## 实现细节

### 搜索位置

调查了以下文件以查找 `session/close` 处理的任何存在：

| 文件 | 角色 | 发现 |
|------|------|---------|
| `apps/acp/src/stdio_loop.rs` | ACP stdio 循环入口点 | 无 `session/close` 处理器 |
| `apps/acp/src/agent.rs` | Agent 请求路由 | 无 `session/close` 路由 |
| `apps/acp/src/session.rs` | Session 生命周期管理 | 包含 `cancel_all_generations()` 但无 ACP 请求处理器 |
| `apps/acp/src/session_config_store.rs` | Session 配置持久化 | 包含 `delete_session()` 仅用于连接级清理 |

### 代码引用

**`session_config_store.rs:112`** — `delete_session()`：
```rust
// 仅连接级清理 — 不是 ACP 协议处理器
pub fn delete_session(&mut self, session_id: &str) -> Result<(), ...> { ... }
```

**`session.rs:209`** — `cancel_all_generations()`：
```rust
// 取消进行中的 LLM 生成 — 不是 ACP session/close 请求处理器
pub fn cancel_all_generations(&mut self) { ... }
```

两个函数都执行内部清理操作，但未连接到 `session/close` 的任何 ACP 请求处理器。

## 实现方式

没有实现。ACP 层中不存在处理函数、路由注册、请求解析和响应序列化。

## 差距与问题

1. **没有请求处理器** — 任何 ACP 模块中都不存在 `session/close` 请求处理器函数。
2. **没有路由注册** — ACP 路由器未将 `session/close` 注册为有效的协议方法。
3. **没有协议解析** — ACP 消息解析层不处理 `session/close` 协议消息。
4. **内部函数具有误导性** — `session_config_store.rs` 中的 `delete_session()` 和 `session.rs` 中的 `cancel_all_generations()` 作为内部清理实用程序存在，但未公开为 ACP 协议处理器，这可能造成混淆。

## 验证

通过使用结构化代码搜索在已确认的文件上执行对抗性验证。该过程确认：

- `session/close` 在 ACP 代码库中**零存在**
- 内部清理函数（`delete_session`、`cancel_all_generations`）是连接级操作，不是 ACP 协议处理器
- 未发现遗漏的实现

**结论：已确认未实现**

## 总结

`session/close` ACP 协议在 Loom 中未实现。要完成此协议：

1. 在 ACP 路由层添加 `session/close` 请求处理器
2. 将其连接到现有的 `delete_session()` / `cancel_all_generations()` 内部函数
3. 在 ACP 消息路由器中注册该路由
4. 在 `tests/acp/` 或 e2e 套件中添加相应的测试覆盖

---

## 实现指南

### 协议规范

`session/close` 是 Client → Agent Request，指示 agent 关闭活动 session。该方法释放关联资源、取消 in-flight 操作，并清理连接状态。**重要区别于 `session/delete`**：close 仅终止活动 session 但保留 session 记录（用于 resume），而 delete 完全从注册表中移除。

- **方向：** Client → Agent
- **请求类型：** `CloseSessionRequest`（仅 `session_id: String`）
- **响应类型：** `CloseSessionResponse`（空响应 `{}`）
- **方法 ID：** `"session/close"`
- **幂等性：** 是 — 重复关闭已关闭的 session 不应报错

### 涉及的类型

```rust
// apps/acp/src/protocol.rs
pub const SESSION_CLOSE: &str = "session/close";

use agent_client_protocol::schema::{CloseSessionRequest, CloseSessionResponse};
```

### Handler 骨架

```rust
// apps/acp/src/agent.rs
pub async fn handle_session_close(
    &self,
    req: CloseSessionRequest,
) -> Result<CloseSessionResponse, AgentError> {
    // 1. 验证 session 存在
    if !self.session_store.session_exists(&req.session_id) {
        // 幂等：未知 session 视为已关闭
        return Ok(CloseSessionResponse::default());
    }

    // 2. 取消该 session 的所有 in-flight 生成
    self.session_store.cancel_generation(&req.session_id).await?;

    // 3. 关闭该 session 的所有活动终端
    self.terminal_manager.kill_by_session(&req.session_id).await?;

    // 4. 标记 session 为关闭状态（保留记录以供 resume）
    self.session_store.mark_closed(&req.session_id)?;

    // 5. 清理 MCP 连接（释放 socket）
    self.mcp_manager.disconnect_session(&req.session_id).await?;

    Ok(CloseSessionResponse::default())
}
```

### 协议路由

在 `apps/acp/src/stdio_loop.rs` 中添加：

```rust
"session/close" => {
    let req: CloseSessionRequest = serde_json::from_value(params)?;
    self.agent.handle_session_close(req).await?
}
```

### 演示：JSON-RPC 请求/响应

**请求（Client → Agent）：**
```json
{
  "jsonrpc": "2.0",
  "id": 20,
  "method": "session/close",
  "params": {
    "session_id": "sess-abc-123"
  }
}
```

**响应（Agent → Client）：**
```json
{
  "jsonrpc": "2.0",
  "id": 20,
  "result": {}
}
```

**幂等调用（已关闭的 session）：**
```text
Client: session/close (id=X)  → 200 OK
  (等待 5 分钟)
Client: session/close (id=X)  → 200 OK（幂等，不报错）
```

### 演示：与 session/cancel 和 session/delete 的对比

```text
session/cancel: 取消 in-flight 操作，session 保持活动
session/close:  终止活动状态 + 取消生成 + 关闭终端，保留 session 记录
session/delete: 从存储中完全删除 session 记录（不可恢复）
```

| 操作 | 取消生成 | 关闭终端 | 清理 MCP | 保留 session 记录 |
|------|---------|---------|---------|------------------|
| `session/cancel` | ✅ | ❌ | ❌ | ✅（活动） |
| `session/close` | ✅ | ✅ | ✅ | ✅（已关闭） |
| `session/delete` | ✅ | ✅ | ✅ | ❌（已删除） |

### 测试场景

在 `apps/acp/tests/e2e_mega.rs` 中添加：

```rust
#[tokio::test]
async fn test_session_close_terminates_active_state() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("test close").await?;

    // 1. 启动 in-flight 操作（后台运行）
    let long_prompt = client.session_prompt_async(
        session_id.clone(),
        "count to 1000000"
    );

    // 2. 立即关闭
    client.session_close(session_id.clone()).await?;

    // 3. 验证：in-flight 操作被取消
    let result = long_prompt.await;
    assert!(matches!(result, Err(_) | Ok(StopReason::Cancelled)));
}

#[tokio::test]
async fn test_session_close_preserves_record() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("preserve test").await?;

    // 1. 关闭
    client.session_close(session_id.clone()).await?;

    // 2. 验证：session 仍然存在（可 resume）
    assert!(client.session_exists(session_id.clone()).await?);

    // 3. 验证：可以 resume
    client.session_resume(session_id).await?;
}

#[tokio::test]
async fn test_session_close_idempotent() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("idempotent test").await?;

    // 1. 第一次 close
    client.session_close(session_id.clone()).await?;

    // 2. 第二次 close（应不报错）
    let result = client.session_close(session_id).await;
    assert!(result.is_ok(), "session/close should be idempotent");
}

#[tokio::test]
async fn test_session_close_kills_terminals() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("terminal test").await?;

    // 1. 创建长运行的终端
    let terminal_id = client.terminal_create(session_id.clone(),
        TerminalCreateRequest {
            command: "sleep".into(),
            args: vec!["60".into()],
            cwd: None, env: Default::default(),
        }
    ).await?;

    // 2. 关闭 session
    client.session_close(session_id).await?;

    // 3. 验证：终端被终止
    let status = client.terminal_wait_for_exit(terminal_id,
        Some(Duration::from_secs(2))).await?;
    assert!(!status.is_running);
}
```

### 验收清单

- [ ] `protocol.rs` 中声明 `SESSION_CLOSE` 常量
- [ ] `agent.rs` 中实现 `handle_session_close` 函数
- [ ] `agent.rs` 中实现 `session_store.mark_closed` 状态标记
- [ ] `agent.rs` 中实现 `terminal_manager.kill_by_session` 批量终止
- [ ] `stdio_loop.rs` 中注册 `"session/close"` 路由
- [ ] 验证幂等性：重复 close 不报错
- [ ] 验证保留 session 记录（与 delete 区别）
- [ ] 验证关闭所有 in-flight 终端和生成
- [ ] 添加 `e2e_mega.rs` 测试用例（4 个：终止活动、保留记录、幂等、终止终端）
