# ACP 协议审计：logout

## 协议规范

`logout` 协议是 Client → Agent 请求，结束当前的认证会话，清理 client 和 agent 之间的已认证状态。

## 实现状态

**未实现** — `logout` ACP 方法在 Loom 代码库中完全缺失。没有处理器注册、没有导入、没有方法、没有协议文档。

## 实现细节

对所有 `.rs` 文件的穷尽搜索确认对 `logout` 零引用：

- `apps/acp/src/agent.rs` — 已确认，无 `logout` 处理器
- `apps/acp/src/stdio_loop.rs` — 已确认，无 `logout` 处理
- `apps/acp/src/protocol.rs` — 已确认，无 `logout` 方法注册
- 工作区中所有其他 `.rs` 文件 — 已确认，无 `logout` 实现

无处理器结构体、无分派分支、无导入、无协议文档。

## 实现方式

N/A — 不存在实现。

## 差距与问题

1. **缺少处理器** — ACP 分派器中未注册 `logout` 方法处理器
2. **缺少协议文档** — `protocol.rs` 方法表中无 `logout` 条目
3. **缺少 agent 逻辑** — `agent.rs` 中没有认证状态清理逻辑
4. **缺少 IPC/stdio 集成** — `stdio_loop.rs` 中没有 `logout` 路径

## 验证

通过以下方式执行对抗性验证：
1. 在所有 `.rs` 文件中搜索 `logout` 关键字
2. 逐行检查已确认的文件（`agent.rs`、`stdio_loop.rs`、`protocol.rs`）
3. 交叉检查 ACP 分派器注册以查找缺失的方法条目

结果：**未实现** — 所有差距均已确认。该搜索在所有 `.rs` 文件中是穷尽的。未检测到遗漏的实现。

## 总结

`logout` ACP 协议方法完全未实现。要完成此功能：

1. 在 `apps/acp/src/protocol.rs` 分派器中添加 `logout` 处理器
2. 在 `apps/acp/src/agent.rs` 中实现 session/auth 清理
3. 在 `apps/acp/src/stdio_loop.rs` 中连接 `logout` IPC 路径
4. 将 `logout` 添加到 ACP 协议规范文档

这是一个合法的缺失功能，而非部分实现。

---

## 实现指南

### 协议规范

`logout` 是 Client → Agent 请求，用于结束当前认证会话、清理 client 和 agent 之间的已认证状态。该方法对每个已认证的 client 一次性调用，调用后 agent 进入未认证状态（等价于未调用过 `authenticate`）。

- **方向：** Client → Agent
- **请求类型：** `LogoutRequest`（空结构体，无字段）
- **响应类型：** `LogoutResponse`（空响应 `{}`）
- **方法 ID：** `"logout"`
- **前置条件：** 客户端应已通过 `authenticate` 完成认证

### 涉及的类型

```rust
// apps/acp/src/protocol.rs
pub const LOGOUT: &str = "logout";

// 从 agent_client_protocol crate 导入（v0.15.1）
use agent_client_protocol::schema::{LogoutRequest, LogoutResponse};
```

### Handler 骨架

```rust
// apps/acp/src/agent.rs — 添加到现有 handlers
pub async fn handle_logout(
    &self,
    _req: LogoutRequest,
) -> Result<LogoutResponse, AgentError> {
    // 1. 取消所有进行中的 LLM 生成
    self.session_store.cancel_all_generations();

    // 2. 关闭所有活动终端
    self.terminal_manager.kill_all().await?;

    // 3. 清理 session 配置存储（不删除 session 记录本身）
    self.session_config_store.clear_auth().await?;

    // 4. 重置 agent 认证状态
    self.reset_auth_state().await;

    Ok(LogoutResponse::default())
}
```

### 协议路由

在 `apps/acp/src/stdio_loop.rs` 的 `match method` 块中添加：

```rust
"logout" => self.agent.handle_logout(req.try_into()?).await?,
```

并在 `apps/acp/src/agent.rs` 的命令注册表中添加：

```rust
define_acp_method!(agent, LOGOUT, handle_logout);
```

### 演示：JSON-RPC 请求/响应

**请求（Client → Agent）：**
```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "method": "logout",
  "params": {}
}
```

**响应（Agent → Client）：**
```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": {}
}
```

**错误响应示例（终端清理失败）：**
```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "error": {
    "code": -32603,
    "message": "internal error: failed to kill terminal abc-123"
  }
}
```

### 演示：完整的认证生命周期

```text
1. initialize        → capability 协商
2. authenticate      → client 通过 AuthMethodId 获取凭证
3. (正常使用 session/prompt 等)
4. logout            → 清理所有认证状态 ← 本协议
5. (可选) authenticate → 重新认证
```

### 测试场景

在 `apps/acp/tests/e2e_mega.rs` 中添加：

```rust
#[tokio::test]
async fn test_logout_clears_auth_state() {
    // 1. 初始化 + 认证
    let client = TestClient::connect().await?;
    let session_id = client.initialize().await?;
    client.authenticate("api_key", "test-key").await?;
    client.session_new(session_id.clone()).await?;

    // 2. 设置一些认证状态
    client.session_prompt(session_id.clone(), "test prompt").await?;
    assert!(client.is_authenticated().await?);

    // 3. 调用 logout
    client.logout().await?;

    // 4. 验证认证状态已清理
    assert!(!client.is_authenticated().await?);

    // 5. 验证 session 仍然存在（logout 不删除 session）
    assert!(client.session_exists(session_id).await?);
}

#[tokio::test]
async fn test_logout_kills_active_terminals() {
    let client = TestClient::connect().await?;
    let session_id = client.initialize().await?;
    let terminal_id = client.terminal_create(session_id.clone(),
        TerminalCreateRequest { command: "sleep".into(), args: vec!["30".into()], ..Default::default() }
    ).await?;

    client.logout().await?;

    // 验证终端已被终止
    let status = client.terminal_wait_for_exit(terminal_id, Some(Duration::from_secs(1))).await?;
    assert!(!status.is_running);
}
```

### 验收清单

- [ ] `apps/acp/src/protocol.rs` 中声明 `LOGOUT` 常量
- [ ] `apps/acp/src/agent.rs` 中实现 `handle_logout` 函数
- [ ] `apps/acp/src/stdio_loop.rs` 中注册 `"logout"` 路由
- [ ] `apps/acp/src/agent.rs` 中通过 `define_acp_method!` 连接处理器
- [ ] 清理 session_store、terminal_manager、session_config_store 的认证状态
- [ ] 验证幂等性：重复调用 logout 不应报错
- [ ] 添加 `e2e_mega.rs` 测试用例（基础清理 + 终端终止）
- [ ] 验证请求/响应 JSON 格式符合 ACP 规范（空 `params` 和空 `result`）
