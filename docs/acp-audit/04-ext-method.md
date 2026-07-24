# ACP 协议审计：extMethod

## 协议规范

`extMethod` 是双向的 Request 协议，用于自定义扩展请求，其中方法名必须以下划线（`_`）开头。它允许 agent 发送带有字符串键方法名的任意扩展方法请求，从而实现向前兼容和插件式可扩展性，无需预定义每个可能的方法。

## 实现状态

**未实现**

## 实现细节

### 协议定义

- **`protocols.lua:77`** — 存在协议注册项：
  ```lua
  extMethod = { "_.*", "ExtMethod" }
  ```
  声明 `extMethod` 为模式匹配协议，正则表达式为 `_.*`，结构体名为 `ExtMethod`。

### 文档引用（已失效）

- **`docs/opencode-protocol/archive/2025-snapshots/acp-adjacent/rust-agent-client-protocol-index.md:266–272`** — 将 `extMethod` 文档化为有效的入站请求处理器。
- **`docs/acp-audit/04-ext-method.md`** — **已被引用但不存在。** `docs/acp-audit/` 目录本身在代码库中缺失。

### Agent 实现（仅骨架）

- **`apps/acp/src/agent.rs:349–1416`** — agent 文件包含 `ExtMethod` 结构体（约第 349 行），但仅作为反向 RPC（Agent→Client）兜底分类。没有处理器、没有路由逻辑、没有入站请求处理。

### 协议层

- **`apps/acp/src/protocol.rs:1–129`** — 此处未路由 `ExtMethod`。没有 `extMethod` 的 protocol.rs 文档块。

### 端到端测试

- **`apps/acp/tests/e2e/common/jsonrpc.rs:72–86`** — `ExtMethod(String)` 变体仅作为出站消息的 JSON-RPC 响应/通知分类存在，而非入站请求处理器。

## 实现方式

不存在实现。该协议在 `protocols.lua` 中以模式注册，但没有处理函数、agent 主循环中的路由 case，也没有测试覆盖。`jsonrpc.rs` 中的 `ExtMethod(String)` 类型仅用于出站消息分类（Agent→Client），不能用于接收入站的 `_` 前缀请求。

## 差距与问题

| 差距 | 严重程度 | 详情 |
|-----|----------|------|
| 缺少处理器 | **严重** | `agent.rs` 中没有 `handle_ext_method` 或等效函数。模式 `_.*` 从未与入站请求匹配。 |
| 无路由逻辑 | **严重** | agent 的分派表/模式匹配不将 `extMethod` 路由到任何实现。 |
| 缺少 `docs/acp-audit/04-ext-method.md` | **高** | 该文档文件在 `protocols.lua:77` 中被引用，但整个 `docs/acp-audit/` 目录缺失。 |
| 无测试 | **高** | 对 `extMethod` 入站处理零 e2e 或单元测试。 |
| ExtMethod 类型是单向的 | **中** | `jsonrpc.rs` 中的 `ExtMethod(String)` 仅用于出站；不能作为入站请求的处理器原型。 |

## 验证

通过交叉引用以下 5 个已确认的文件执行对抗性验证：

1. **`protocols.lua:77`** — 已确认 `extMethod` 注册项，模式为 `_.*`。
2. **`docs/opencode-protocol/archive/2025-snapshots/acp-adjacent/rust-agent-client-protocol-index.md:266–272`** — 已确认将 `extMethod` 列为入站处理器的文档。
3. **`apps/acp/src/agent.rs:349–1416`** — 已确认不存在处理函数；`ExtMethod` 结构体存在但未用作入站处理器。
4. **`apps/acp/src/protocol.rs:1–129`** — 已确认无 `extMethod` 路由或文档。
5. **`apps/acp/tests/e2e/common/jsonrpc.rs:72–86`** — 已确认 `ExtMethod(String)` 仅用于出站分类。

**结论：已验证 — 协议未实现。** 分析准确。`extMethod` 在协议注册表和文档中已定义，但没有处理器、没有路由逻辑、没有 protocol.rs 文档，也没有测试。

## 总结

`extMethod` 是个**存根协议**。注册表条目和类型定义存在，但实际请求处理未连接。要实现此协议：

1. 在 `agent.rs` 中添加处理器函数 `handle_ext_method(method: String, params: Value)`。
2. 在 agent 的分派逻辑中，将模式 `_.*` 路由匹配入站请求到新处理器。
3. 添加 `docs/acp-audit/04-ext-method.md` 以记录规范（即本文档）。
4. 在 `apps/acp/tests/e2e/` 中添加 e2e 测试，至少覆盖有效的 `_` 前缀方法路由和错误处理。

**未修复的风险：** 任何入站的 `_` 前缀 ACP 请求将被静默丢弃或导致"未知方法"错误，破坏扩展协议的向前兼容性。

---

## 实现指南

### 协议规范

`extMethod` 是双向 Request 协议，用于自定义扩展方法。**关键约束：方法名必须以下划线（`_`）开头。** 这允许任何一方发送不映射到已知协议方法的任意扩展请求，从而启用插件式可扩展性而无需预定义每个可能的方法。

- **方向：** 双向（Client ↔ Agent）
- **方法名模式：** `^_[A-Za-z0-9_]+$`（以 `_` 开头，可含字母数字和下划线）
- **请求类型：** `ExtMethod`（包含 `method: String` 和 `params: Value`）
- **响应类型：** `ExtMethodResponse`（包含 `result: Value`）
- **错误处理：** 未知扩展方法返回 `-32601`（方法未找到）

### 涉及的类型

```rust
// apps/acp/src/protocol.rs
pub const EXT_METHOD_PATTERN: &str = r"^_[A-Za-z0-9_]+$";

// 从 agent_client_protocol crate 导入
use agent_client_protocol::schema::{ExtMethod, ExtMethodResponse};
```

### Handler 骨架

```rust
// apps/acp/src/agent.rs
pub async fn handle_ext_method(
    &self,
    req: ExtMethod,
) -> Result<ExtMethodResponse, AgentError> {
    // 1. 验证方法名以 _ 开头
    if !req.method.starts_with('_') {
        return Err(AgentError::InvalidMethod(req.method));
    }

    // 2. 路由到注册的扩展处理器
    if let Some(handler) = self.ext_handlers.read().await.get(&req.method) {
        let result = handler(req.params).await?;
        Ok(ExtMethodResponse { result })
    } else {
        // 3. 未注册的扩展方法返回未找到
        Err(AgentError::MethodNotFound(req.method))
    }
}
```

### 扩展注册 API

```rust
// apps/acp/src/agent.rs — 公开 API
impl Agent {
    pub fn register_ext_method<F, Fut>(&self, method: &str, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, AgentError>> + Send,
    {
        assert!(method.starts_with('_'), "extMethod must start with underscore");
        self.ext_handlers.write().await.insert(method.to_string(),
            Arc::new(move |params| Box::pin(handler(params))));
    }
}
```

### 协议路由

在 `apps/acp/src/stdio_loop.rs` 的 `match method` 块中，**在所有精确匹配之后**添加 fallback 分支：

```rust
// stdio_loop.rs — 主分派器
match method {
    "initialize" => self.handle_initialize(...).await?,
    "session/new" => self.handle_session_new(...).await?,
    "session/prompt" => self.handle_prompt(...).await?,
    // ... 其他精确匹配 ...
    _ if method.starts_with('_') => {
        // extMethod fallback
        self.agent.handle_ext_method(ExtMethod {
            method: method.to_string(),
            params: req.params,
        }).await?
    }
    _ => return Err(AgentError::MethodNotFound(method.into())),
}
```

### 演示：JSON-RPC 请求/响应

**客户端发送自定义方法 `_echo`：**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "_echo",
  "params": { "message": "hello" }
}
```

**Agent 响应：**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": { "echoed": "hello" }
}
```

**未注册方法错误：**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "error": {
    "code": -32601,
    "message": "method not found: _unknown_method"
  }
}
```

**方法名格式错误（不以 `_` 开头）：**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "error": {
    "code": -32600,
    "message": "invalid method: must start with underscore"
  }
}
```

### 演示：双向扩展

```text
Client → Agent:  _capabilities/list    → Agent 返回自定义能力
Client → Agent:  _telemetry/event      → Agent 接收遥测
Agent → Client:  _progress/update      → Client 接收进度（反向 RPC）
```

### 演示：注册自定义扩展

```rust
// 在 agent 启动时
agent.register_ext_method("_echo", |params| async move {
    Ok(params)  // echo back the params
});

agent.register_ext_method("_metrics", |_params| async move {
    Ok(serde_json::json!({
        "active_sessions": 3,
        "uptime_seconds": 3600,
    }))
});
```

### 测试场景

在 `apps/acp/tests/e2e_mega.rs` 中添加：

```rust
#[tokio::test]
async fn test_ext_method_underscore_prefix() {
    let client = TestClient::connect().await?;

    // 1. 注册测试扩展
    client.register_ext_method("_test_echo", |params| async move {
        Ok(params)
    }).await;

    // 2. 发送扩展方法调用
    let result: Value = client.call("_test_echo", json!({ "x": 1 })).await?;
    assert_eq!(result, json!({ "x": 1 }));
}

#[tokio::test]
async fn test_ext_method_unregistered_fails() {
    let client = TestClient::connect().await?;

    // 1. 尝试调用未注册的方法
    let result = client.call::<_, Value>("_nonexistent", json!({})).await;

    // 2. 验证返回方法未找到错误
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, -32601);
}

#[tokio::test]
async fn test_ext_method_validates_underscore_prefix() {
    let client = TestClient::connect().await?;

    // 1. 尝试调用不以 _ 开头的方法（应路由到未知方法而非 extMethod）
    let result = client.call::<_, Value>("no_underscore", json!({})).await;

    // 2. 应该被识别为非 extMethod
    assert!(result.is_err());
}
```

### 验收清单

- [ ] `protocol.rs` 中定义 `EXT_METHOD_PATTERN` 正则
- [ ] `agent.rs` 中实现 `handle_ext_method` 函数
- [ ] `agent.rs` 中实现 `register_ext_method` 公开 API
- [ ] `stdio_loop.rs` 中在精确匹配之后添加 `_` 前缀 fallback
- [ ] 验证下划线前缀：非 `_` 开头的方法不应路由到 extMethod 处理器
- [ ] 验证未注册方法返回 `-32601`（方法未找到）JSON-RPC 错误
- [ ] 验证双向支持：Client→Agent 和 Agent→Client（反向 RPC）
- [ ] 添加 `e2e_mega.rs` 测试用例（3 个：成功调用、未注册错误、前缀验证）
- [ ] 验证无副作用的 handler 隔离（handler panic 不应影响其他协议）
