# ACP 协议审计：session/delete

## 协议规范

`session/delete` 协议定义了 Client → Agent 请求，用于从 session 列表中删除 session。此操作允许 client 从 agent 维护的活动 session 注册表中删除不需要的、已完成的或过时的 session。

## 实现状态

**未实现**

Loom 当前未实现 `session/delete` 协议处理器。

## 实现细节

对抗性验证确认已检查以下文件位置：

| 文件 | 行 | 角色 |
|------|-------|------|
| `apps/acp/src/stdio_loop.rs` | 28-51, 280-441 | ACP stdio 循环；未找到 session/delete 处理器 |
| `apps/acp/src/agent.rs` | 384-386 | Agent 请求路由；session/delete 未路由 |
| `apps/acp/src/session.rs` | 116-266 | Session 管理；未实现 delete 操作 |
| `apps/acp/src/session_config_store.rs` | 112-118 | Session 配置持久化；无 delete 方法 |
| `protocols.lua` | 86 | 仅协议清单引用 |
| `docs/rust-agent-client-protocol-index.md` | 370-376 | 协议文档条目 |

该协议在 `protocols.lua` 中列出并在协议索引中记录，但任何 session 管理模块中都没有相应的处理器、命令或 delete 逻辑。

## 实现方式

由于协议未实现，因此没有要记录的实现方式。Session 管理子系统（`session.rs`）提供 session 创建、加载和列出操作，但不包括 delete 功能。

## 差距与问题

- **缺少处理器**：`agent.rs` 请求路由（第 384-386 行附近）中没有 `session_delete` 或等效函数。
- **缺少 repository 方法**：`session_config_store.rs` 没有 delete 操作；无法从持久化存储中删除 session。
- **缺少 CLI 命令**：`stdio_loop.rs` 未向用户公开 `session delete` 命令。
- **协议已列出但未实现**：该协议出现在 `protocols.lua:86` 和索引文档（`docs/rust-agent-client-protocol-index.md:370-376`）中，但没有任何实现。

## 验证

通过以下方式执行对抗性验证：
1. 在所有 session 管理文件中搜索 delete 相关逻辑
2. 检查 `agent.rs` 中的 ACP 请求路由以查找 session/delete 分派
3. 检查 `session_config_store.rs` 中的 delete/remove 方法
4. 审查 `protocols.lua` 协议清单条目
5. 与协议索引文档交叉引用

**结果**：`session/delete` 已确认**未实现**。未找到不同名称的替代实现。

## 总结

`session/delete` 协议在 ACP 协议清单中声明，但在 Loom 中没有运行时实现。要完成此协议：

1. 将 `delete_session(session_id)` 方法添加到 `SessionConfigStore`
2. 在 `agent.rs` 请求路由中实现 ACP 请求处理器
3. 在 `stdio_loop.rs` 中公开 `session delete <id>` CLI 命令
4. 更新协议索引文档以反映实现状态

优先级：**低** — 似乎没有活动消费者依赖此协议。

---

## 实现指南

### 协议规范

`session/delete` 是 Client → Agent Request，用于**从 session 注册表中完全移除** session 记录。这是不可逆操作，区别于 `session/close`（保留记录用于 resume）。

- **方向：** Client → Agent
- **请求类型：** `DeleteSessionRequest`（仅 `session_id: String`）
- **响应类型：** `DeleteSessionResponse`（空响应 `{}`）
- **方法 ID：** `"session/delete"`
- **幂等性：** 是 — 重复删除已删除的 session 不应报错
- **不可逆：** 删除后无法 resume；session 数据从持久化存储中擦除

### 涉及的类型

```rust
// apps/acp/src/protocol.rs
pub const SESSION_DELETE: &str = "session/delete";

use agent_client_protocol::schema::{DeleteSessionRequest, DeleteSessionResponse};
```

### Handler 骨架

```rust
// apps/acp/src/agent.rs
pub async fn handle_session_delete(
    &self,
    req: DeleteSessionRequest,
) -> Result<DeleteSessionResponse, AgentError> {
    // 1. 幂等检查：未知 session 视为已删除
    if !self.session_store.session_exists(&req.session_id) {
        return Ok(DeleteSessionResponse::default());
    }

    // 2. 先调用 session/close 语义（取消生成、关闭终端、清理 MCP）
    self.session_store.cancel_generation(&req.session_id).await?;
    self.terminal_manager.kill_by_session(&req.session_id).await?;
    self.mcp_manager.disconnect_session(&req.session_id).await?;

    // 3. 从 session 存储中删除
    self.session_store.delete(&req.session_id)?;

    // 4. 从 session_config_store 中删除（清理持久化配置）
    self.session_config_store.delete_session(&req.session_id)?;

    // 5. 从 L1/DashMap 缓存中清除
    self.session_cache.remove(&req.session_id);

    Ok(DeleteSessionResponse::default())
}
```

### 协议路由

在 `apps/acp/src/stdio_loop.rs` 中添加：

```rust
"session/delete" => {
    let req: DeleteSessionRequest = serde_json::from_value(params)?;
    self.agent.handle_session_delete(req).await?
}
```

### 演示：JSON-RPC 请求/响应

**请求（Client → Agent）：**
```json
{
  "jsonrpc": "2.0",
  "id": 30,
  "method": "session/delete",
  "params": {
    "session_id": "sess-abc-123"
  }
}
```

**响应（Agent → Client）：**
```json
{
  "jsonrpc": "2.0",
  "id": 30,
  "result": {}
}
```

**幂等调用：**
```text
Client: session/delete (id=X)  → 200 OK
  (1 秒后)
Client: session/delete (id=X)  → 200 OK（幂等）
```

### 演示：与 session/close 的对比

```text
session/close + session/delete（两步）：
  1. session/close   → 终止活动，保留记录
  2. session/delete  → 从存储中完全移除

session/delete（一步）：
  → 一步完成 close + delete 等效操作
```

| 操作 | 终止活动 | 保留记录 | 可恢复 |
|------|---------|---------|-------|
| `session/close` | ✅ | ✅ | ✅（通过 resume） |
| `session/delete` | ✅ | ❌ | ❌（永久删除） |

### 演示：完整清理流程

```text
1. session/new (id=X)              ← 创建
2. session/prompt (id=X, "hi")     ← 使用
3. terminal/create (id=X, "sleep") ← 创建终端
4. session/delete (id=X)           ← 一次性清理：
   ├─ cancel generation
   ├─ kill terminals
   ├─ disconnect MCP
   ├─ remove from session_store
   ├─ remove from session_config_store
   └─ evict from L1 cache
```

### 测试场景

在 `apps/acp/tests/e2e_mega.rs` 中添加：

```rust
#[tokio::test]
async fn test_session_delete_removes_from_registry() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("test delete").await?;

    // 1. 验证 session 存在
    assert!(client.session_exists(session_id.clone()).await?);
    let sessions_before = client.session_list().await?;
    assert!(sessions_before.iter().any(|s| s.session_id == session_id));

    // 2. 删除
    client.session_delete(session_id.clone()).await?;

    // 3. 验证：session 已从注册表中移除
    assert!(!client.session_exists(session_id.clone()).await?);
    let sessions_after = client.session_list().await?;
    assert!(!sessions_after.iter().any(|s| s.session_id == session_id));
}

#[tokio::test]
async fn test_session_delete_terminates_resources() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("resources test").await?;

    // 1. 创建终端
    let terminal_id = client.terminal_create(session_id.clone(),
        TerminalCreateRequest {
            command: "sleep".into(),
            args: vec!["60".into()],
            cwd: None, env: Default::default(),
        }
    ).await?;

    // 2. 删除 session
    client.session_delete(session_id).await?;

    // 3. 验证：终端被终止
    let status = client.terminal_wait_for_exit(terminal_id,
        Some(Duration::from_secs(2))).await?;
    assert!(!status.is_running);
}

#[tokio::test]
async fn test_session_delete_idempotent() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("idempotent delete").await?;

    // 1. 第一次删除
    client.session_delete(session_id.clone()).await?;

    // 2. 第二次删除（应不报错）
    let result = client.session_delete(session_id).await;
    assert!(result.is_ok(), "session/delete should be idempotent");
}

#[tokio::test]
async fn test_session_delete_clears_persistent_config() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("config test").await?;

    // 1. 设置持久化配置
    client.set_config_option(session_id.clone(), "model", "claude-opus").await?;
    assert_eq!(
        client.get_config_option(session_id.clone(), "model").await?,
        "claude-opus"
    );

    // 2. 删除
    client.session_delete(session_id.clone()).await?;

    // 3. 验证：配置已从持久化存储中清除
    // （如果重新创建同名 session，应得到默认配置）
    let new_id = client.session_new("after delete").await?;
    let model = client.get_config_option(new_id, "model").await?;
    assert_ne!(model, "claude-opus", "config should be cleared after delete");
}

#[tokio::test]
async fn test_session_delete_unknown_session() {
    let client = TestClient::connect().await?;

    // 1. 删除不存在的 session（应幂等成功）
    let result = client.session_delete("sess-nonexistent").await;
    assert!(result.is_ok());
}
```

### 验收清单

- [ ] `protocol.rs` 中声明 `SESSION_DELETE` 常量
- [ ] `agent.rs` 中实现 `handle_session_delete` 函数
- [ ] `agent.rs` 中实现 `session_store.delete` 完整移除
- [ ] `agent.rs` 中实现 `session_config_store.delete_session` 清理持久化
- [ ] `agent.rs` 中从 L1/DashMap 缓存中清除
- [ ] `stdio_loop.rs` 中注册 `"session/delete"` 路由
- [ ] 验证幂等性：重复 delete 不报错
- [ ] 验证完整清理：session、配置、缓存、终端、MCP
- [ ] 验证 session/delete 不可恢复：删除后无法 resume
- [ ] 添加 `e2e_mega.rs` 测试用例（5 个：移除注册表、终止资源、幂等、清理配置、未知 session）
