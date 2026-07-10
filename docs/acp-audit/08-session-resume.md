# ACP 协议审计：session/resume

## 协议规范

`session/resume` 协议允许 client 在不重放完整历史的情况下恢复现有 session。与 `session/load`（加载并回放历史）不同，resume 提供到活动或暂停的 agent session 的轻量级重连，传递 session 的状态而无需重新执行先前的步骤。这是 Client → Agent 方向。

## 实现状态

**未实现。** `session/resume` ACP 协议在 Loom 代码库中缺失。

## 实现细节

未找到实现。所有五类证据均被检查并返回负面结果：

1. **能力未声明** — `agent.rs:378-395`（initialize 处理器）未声明 `session/resume` 能力。
2. **没有 `resume_session` 函数** — 在 `agent.rs` 中 grep 确认不存在 `resume_session` async fn。
3. **没有 resume 类型** — `ResumeSessionRequest` / `ResumeSessionResponse` 在任何 `.rs` 文件中均不存在。
4. **没有协议条目** — `protocol.rs:81-106` 不包括 `session/resume` 条目。
5. **没有特性标志** — `Cargo.toml:29` 没有 `unstable_resume_session` 特性。

`session/load` 能力在 `agent.rs:1048` 实现，处理加载+回放历史 — 这是一个与 `session/resume` 不同的协议。

注：`agent.rs:1113-1118` 包含 LangGraph 风格的中断恢复字段（`resume_from_node_id`、`resume_value` 等）。这些与 ACP `session/resume` 无关。

## 实现方式

N/A — 协议未实现。

## 差距与问题

- 该协议在 `docs/rust-agent-client-protocol-index.md:522` 中被记录为稳定，但没有任何对应的实现或 crate 特性。
- `session/load` 存在并加载+回放历史，但不提供用于活动/暂停 session 的轻量级 resume 路径。
- 在 ACP 层中未找到 session 级 resume 的替代命名（`restore`、`reconnect` 等）。
- 依赖 `session/resume` 的 client 将收到不支持的协议错误。

## 验证

对抗性验证确认已检查以下文件：

| 证据类别 | 文件 | 行 | 结果 |
|---|---|---|---|
| 初始化能力 | `apps/acp/src/agent.rs` | 378-395 | 未声明 |
| Resume 函数 | `apps/acp/src/agent.rs` | 全部 | 未找到 |
| Resume 类型 | 所有 `.rs` 文件 | — | 未找到 |
| 协议注册表 | `apps/acp/src/protocol.rs` | 81-106 | 不存在 |
| 特性标志 | `apps/acp/Cargo.toml` | 29 | 不存在 |

`agent.rs:1113-1118` 处的 LangGraph 中断恢复字段被明确排除为不相关。

**结论：已确认未实现。**

## 总结

`session/resume` ACP 协议在 Loom 中未实现。已记录的协议规范与代码库不匹配。如果需要轻量级 session resume，则需要从头实现 — 要么作为新的 ACP 协议，要么作为现有 `session/load` 路径的扩展并增加仅 resume 模式。第一步将是把 `resume_session` 添加到 `agent.rs` 并在 `protocol.rs` 中注册该能力。

---

## 实现指南

### 协议规范

`session/resume` 是 Client → Agent Request，**在不回放完整历史的情况下**恢复现有 session。与 `session/load`（加载并流式回放历史）的关键区别：resume 仅重新连接到活动状态，不重新执行先前的步骤。

- **方向：** Client → Agent
- **请求类型：** `ResumeSessionRequest`
  - `session_id: String` — 必填
  - `working_directory: Option<PathBuf>` — 可选工作目录
  - `mcp_servers: Option<Vec<AcpMcpServer>>` — 可选 MCP 服务器
- **响应类型：** `ResumeSessionResponse`（含 `modes`、`config_options`、`meta`）
- **方法 ID：** `"session/resume"`
- **能力声明：** `loadSession: true`（与 session/load 共享能力标志）

### 涉及的类型

```rust
// apps/acp/src/protocol.rs
pub const SESSION_RESUME: &str = "session/resume";

// 从 agent_client_protocol crate 导入
use agent_client_protocol::schema::{ResumeSessionRequest, ResumeSessionResponse};
```

### Handler 骨架

```rust
// apps/acp/src/agent.rs — 关键实现
pub async fn handle_session_resume(
    &self,
    req: ResumeSessionRequest,
) -> Result<ResumeSessionResponse, AgentError> {
    // 1. 验证 session 存在
    if !self.session_store.session_exists(&req.session_id) {
        return Err(AgentError::SessionNotFound(req.session_id.clone()));
    }

    // 2. 获取现有 session 条目（不创建新条目）
    let entry = self.session_store.get(&req.session_id)?;

    // 3. 应用工作目录（如果提供）
    if let Some(cwd) = req.working_directory {
        entry.set_working_directory(cwd);
    }

    // 4. 连接 MCP 服务器（如果提供）
    if let Some(mcp_servers) = req.mcp_servers {
        let loom_mcps = acp_mcp_to_loom(mcp_servers);
        entry.set_mcp_servers(loom_mcps);
    }

    // 5. 重新挂载到 agent 主循环（关键：恢复 in-flight 状态）
    self.attach_to_session(&entry)?;

    // 6. 构造响应（不发送历史 chunk）
    let modes = self.list_modes();
    let config_options = self.build_config_options()?;

    Ok(ResumeSessionResponse {
        modes,
        config_options,
        meta: None,
    })
}
```

### 与 session/load 的差异

```rust
// 共享：会话查找、配置应用
fn session_lookup_common(&self, session_id: &str) -> Result<SessionEntry> {
    if !self.session_store.session_exists(session_id) {
        return Err(AgentError::SessionNotFound(session_id.into()));
    }
    self.session_store.get(session_id)
}

// session/load: 回放历史
pub async fn load_session(&self, req: LoadSessionRequest) -> Result<...> {
    let entry = self.session_lookup_common(&req.session_id)?;
    self.replay_history(&entry)?;  // ← 关键差异
    self.attach_to_session(&entry)?;
    Ok(...)
}

// session/resume: 跳过历史回放
pub async fn resume_session(&self, req: ResumeSessionRequest) -> Result<...> {
    let entry = self.session_lookup_common(&req.session_id)?;
    // 不调用 replay_history()  ← 关键差异
    self.attach_to_session(&entry)?;
    Ok(...)
}
```

### 协议路由

在 `apps/acp/src/stdio_loop.rs:358-370` 附近添加：

```rust
"session/resume" => {
    let req: ResumeSessionRequest = serde_json::from_value(params)?;
    self.agent.handle_session_resume(req).await?
}
```

并在 `apps/acp/src/agent.rs` 的 initialize 处理器中确认能力声明：

```rust
obj.insert(
    "agentCapabilities".to_string(),
    serde_json::json!({
        "loadSession": true,  // resume 共享此能力
        "sessionCapabilities": { "list": {}, "fork": {} },
        "promptCapabilities": { "embeddedContext": true, "image": true, "audio": true }
    }),
);
```

### 演示：JSON-RPC 请求/响应

**请求（Client → Agent）：**
```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "session/resume",
  "params": {
    "session_id": "sess-abc-123",
    "working_directory": "/home/user/project"
  }
}
```

**响应（Agent → Client）：**
```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "result": {
    "modes": [
      { "id": "ask", "name": "Ask", "description": "..." },
      { "id": "dev", "name": "Dev", "description": "..." }
    ],
    "configOptions": [
      { "id": "model", "type": "string", "currentValue": "claude-opus-4-5" }
    ],
    "meta": null
  }
}
```

**错误响应（session 不存在）：**
```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "error": {
    "code": -32004,
    "message": "session not found: sess-abc-123"
  }
}
```

### 演示：session/load vs session/resume 时序

```text
session/load:                              session/resume:
─────────────────────────────────         ─────────────────────────────────
Client: session/load (id=X)               Client: session/resume (id=X)
  │                                         │
  ↓                                         ↓
Agent: 查检查点                                Agent: 查 session 条目
  │                                         │
  ↓                                         ↓
Agent: 发送历史 chunk (N 个)              Agent: 无操作（跳过历史）
  │                                         │
  ↓                                         ↓
Agent: 挂载到主循环                          Agent: 挂载到主循环
  │                                         │
  ↓                                         ↓
Client: 接收历史                              Client: 直接进入 prompt 模式
  │
  ↓
Client: 准备发送新 prompt
```

### 测试场景

在 `apps/acp/tests/e2e_mega.rs` 中添加：

```rust
#[tokio::test]
async fn test_session_resume_reconnects_without_replay() {
    let client = TestClient::connect().await?;

    // 1. 创建并加载 session
    let session_id = client.session_new("test resume").await?;
    client.session_prompt(session_id.clone(), "echo back: hello").await?;

    // 2. 加载以建立状态
    client.session_load(session_id.clone()).await?;

    // 3. 记录历史 chunk 数量
    let pre_resume_chunks = client.received_chunks();

    // 4. Resume（不应重放历史）
    client.session_resume(session_id.clone()).await?;

    // 5. 验证：resume 后未收到额外的历史 chunk
    let post_resume_chunks = client.received_chunks();
    assert_eq!(pre_resume_chunks, post_resume_chunks,
               "resume should not replay history");
}

#[tokio::test]
async fn test_session_resume_unknown_session() {
    let client = TestClient::connect().await?;

    // 1. 尝试 resume 不存在的 session
    let result = client.session_resume("sess-nonexistent").await;

    // 2. 验证错误
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, -32004);
}

#[tokio::test]
async fn test_session_resume_attaches_to_main_loop() {
    let client = TestClient::connect().await?;
    let session_id = client.session_new("test attach").await?;

    // 1. Resume
    client.session_resume(session_id.clone()).await?;

    // 2. 验证：resume 后能正常 prompt（说明已挂载）
    let response = client.session_prompt(session_id, "ping").await?;
    assert!(response.contains("pong") || response.contains("echo"));
}
```

### 验收清单

- [ ] `protocol.rs` 中声明 `SESSION_RESUME` 常量
- [ ] `agent.rs` 中实现 `handle_session_resume` 函数
- [ ] `agent.rs` 中实现 `attach_to_session` 辅助函数（与 session/load 共享）
- [ ] `stdio_loop.rs` 中注册 `"session/resume"` 路由
- [ ] 验证：resume **不**发送历史 `session/update` 通知
- [ ] 验证：未知 session 返回 `-32004`（session not found）错误
- [ ] 验证：resume 后 session 可立即接收新的 `session/prompt`
- [ ] 添加 `e2e_mega.rs` 测试用例（3 个：不重放、未知 session、挂载后 prompt）
- [ ] 与 `session/load` 共享 `session_lookup_common` 辅助函数以减少代码重复
