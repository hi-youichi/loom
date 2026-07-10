# ACP 协议审计：extNotification

## 协议规范

`extNotification` 协议是一个双向的 ACP 通知方法，其方法名带有 `_`（下划线）前缀。它作为不映射到任何已知协议方法的自定义通知的通用扩展点。与承载语义的标准 ACP 方法不同，extNotification 允许各方交换任意自定义通知负载，而无需定义新的协议变体。

## 实现状态

**未实现**

## 实现细节

ACP 代码库包含一个 `ReverseRpcKind::ExtMethod` 变体，专门用于在测试 harness 中对*出站*反向 RPC 方法名进行分类：

**文件：`apps/acp/tests/e2e/common/jsonrpc.rs:65-86`**
```rust
ReverseRpcKind::ExtMethod => {
    // Used only for test classification of non-matching outgoing method names
}
```

该变体**不**提供任何用于处理入站 `_` 前缀通知的处理器、结构体或逻辑。

**文件：`apps/acp/src/agent.rs:516-521`** 和 **`apps/acp/src/stdio_loop.rs:410-421`**

两个位置都只处理 `CancelNotification` 作为入站通知。两个文件中均不存在 extNotification 的处理器或路由逻辑。

| 文件 | 角色 |
|------|------|
| `apps/acp/tests/e2e/common/jsonrpc.rs:72` | `ReverseRpcKind::ExtMethod` 变体 — 仅用于测试 harness 分类 |
| `apps/acp/src/agent.rs:516-521` | 仅 `CancelNotification` 入站通知处理器 |
| `apps/acp/src/stdio_loop.rs:410-421` | 仅 `CancelNotification` 入站通知处理器 |

不存在专用的 `struct`、枚举变体、能力标志或处理器函数用于 `extNotification`。

## 实现方式

N/A — 不存在实现。

## 差距与问题

1. **没有入站 extNotification 处理器** — Loom 没有代码路径来接受、路由或分派从对端接收的 `_` 前缀通知。
2. **`ReverseRpcKind::ExtMethod` 仅用于测试** — 它对测试 harness 中的出站方法名进行分类，未接入到任何实际的分派逻辑。
3. **没有 extNotification 负载的结构体** — 不存在 `ExtNotification`、`ext_notification` 或等效类型来表示通知内容。
4. **没有能力协商** — 没有机制在对等方之间声明或协商对 extNotification 的支持。
5. **没有单元或集成测试** — 两个方向上均无发送或接收 extNotification 的测试覆盖。

## 验证

通过以下方式进行对抗性验证：

1. 在 `apps/acp/src/` 和 `apps/acp/tests/e2e/common/` 中跨代码库搜索所有对 `ExtMethod`、`_` 前缀处理和 extNotification 模式的引用。
2. 检查 `agent.rs:516-521` 和 `stdio_loop.rs:410-421` 的入站通知处理器，确认仅处理 `CancelNotification`。
3. 检查 `jsonrpc.rs:65-86` 的测试 harness，确认 `ReverseRpcKind::ExtMethod` 仅用于分类。

**结论：已确认** — `extNotification` 协议未实现。`ReverseRpcKind::ExtMethod` 存在，但仅作为测试 harness 中的出站分类标签。不存在用于处理来自对端的入站 extNotification 的处理器、结构体、能力或测试。

## 总结

`extNotification` 协议是 Loom ACP 实现中的一个明显差距。如果需要双向自定义通知，则需要以下内容：

- `agent.rs` 或 `stdio_loop.rs` 中的处理器函数以接收 `_` 前缀通知
- 入站分派逻辑中的 `ReverseRpcKind::ExtMethod` 分支（当前仅分派 `CancelNotification`）
- `struct ExtNotification` 或类似结构以反序列化通知负载
- 可选：如果应声明 extNotification 支持，则进行能力协商

**优先级：低** — extNotification 是一个扩展点。除非具体用例需要任意自定义通知，否则此差距可保持不处理。

---

## 实现指南

### 协议规范

`extNotification` 是双向 Notification 协议，用于自定义扩展通知。**关键区别于 `extMethod`：通知是单向的，不期望响应。** 方法名同样必须以下划线（`_`）开头以避免与标准协议方法名冲突。

- **方向：** 双向（Client ↔ Agent）
- **方法名模式：** `^_[A-Za-z0-9_]+$`（与 `extMethod` 相同）
- **类型：** `ExtNotification`（包含 `method: String` 和 `params: Value`）
- **响应：** 无（通知不期望响应）
- **传输：** ACP Notification（无 `id` 字段，因为是单向）

### 涉及的类型

```rust
// apps/acp/src/protocol.rs
pub const EXT_NOTIFICATION_PATTERN: &str = r"^_[A-Za-z0-9_]+$";

use agent_client_protocol::schema::ExtNotification;
```

### Handler 骨架

```rust
// apps/acp/src/agent.rs
pub async fn handle_ext_notification(
    &self,
    req: ExtNotification,
) -> Result<(), AgentError> {
    // 1. 验证方法名以 _ 开头
    if !req.method.starts_with('_') {
        return Err(AgentError::InvalidMethod(req.method));
    }

    // 2. 路由到注册的扩展通知处理器
    if let Some(handler) = self.ext_notification_handlers.read().await.get(&req.method) {
        handler(req.params).await?;
        Ok(())
    } else {
        // 3. 未注册的通知：记录日志但不返回错误（通知语义）
        tracing::warn!(method = %req.method, "received unregistered ext notification");
        Ok(())
    }
}
```

### 扩展注册 API

```rust
// apps/acp/src/agent.rs — 公开 API
impl Agent {
    pub fn register_ext_notification<F, Fut>(&self, method: &str, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), AgentError>> + Send,
    {
        assert!(method.starts_with('_'), "extNotification must start with underscore");
        self.ext_notification_handlers.write().await.insert(
            method.to_string(),
            Arc::new(move |params| Box::pin(handler(params))),
        );
    }

    // 主动发送 extNotification
    pub async fn send_ext_notification(
        &self,
        client: &Client,
        method: &str,
        params: Value,
    ) -> Result<(), AgentError> {
        assert!(method.starts_with('_'), "extNotification must start with underscore");
        client.notify(&ExtNotification { method: method.into(), params }).await
    }
}
```

### 协议路由

在 `apps/acp/src/stdio_loop.rs` 的 notification 分派中添加：

```rust
// stdio_loop.rs — notification 分派器
match method.as_str() {
    "session/cancel" => self.agent.cancel(),
    _ if method.starts_with('_') => {
        // extNotification fallback
        let req: ExtNotification = serde_json::from_value(params)?;
        self.agent.handle_ext_notification(req).await?;
    }
    _ => tracing::warn!(method = %method, "unknown notification"),
}
```

### 演示：JSON-RPC 通知（无 id 字段）

**Client → Agent 通知 `_user_activity`：**
```json
{
  "jsonrpc": "2.0",
  "method": "_user_activity",
  "params": {
    "user_id": "u-123",
    "action": "typing",
    "timestamp": 1692500000
  }
}
```

**Agent → Client 通知 `_model_thought`：**
```json
{
  "jsonrpc": "2.0",
  "method": "_model_thought",
  "params": {
    "thought": "considering file structure",
    "step": 3
  }
}
```

**注意：** 通知**不包含** `id` 字段，且**不期望响应**。

### 演示：注册自定义通知处理器

```rust
// 客户端注册：监听 agent 推送的自定义通知
agent.register_ext_notification("_model_thought", |params| async move {
    if let Some(thought) = params.get("thought").and_then(|v| v.as_str()) {
        println!("[Agent thinking] {}", thought);
    }
    Ok(())
});

// 客户端主动发送：通知 agent 用户活动
client.send_ext_notification("_user_activity", json!({
    "action": "idle",
    "duration_ms": 5000
})).await?;
```

### 演示：与 extMethod 的对比

| 维度 | extMethod | extNotification |
|------|-----------|-----------------|
| 方向 | Request-Response | 单向（fire-and-forget） |
| 方法名前缀 | `_` 开头 | `_` 开头 |
| 包含 `id` 字段 | 是（用于匹配响应） | 否 |
| 期望响应 | 是 | 否 |
| 未注册时行为 | 返回 `-32601` 错误 | 静默忽略（仅记录日志） |
| 典型用例 | 自定义 RPC | 进度、心跳、活动跟踪 |

### 测试场景

在 `apps/acp/tests/e2e_mega.rs` 中添加：

```rust
#[tokio::test]
async fn test_ext_notification_received() {
    let client = TestClient::connect().await?;
    let received = Arc::new(Mutex::new(None));
    let r = received.clone();

    // 1. 注册通知处理器
    client.register_ext_notification("_test_event", move |params| {
        let r = r.clone();
        async move {
            *r.lock().await = Some(params);
            Ok(())
        }
    }).await;

    // 2. 发送通知
    client.notify("_test_event", json!({ "value": 42 })).await?;

    // 3. 等待通知被接收
    tokio::time::timeout(Duration::from_secs(1), async {
        while received.lock().await.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await?;

    // 4. 验证
    assert_eq!(received.lock().await.as_ref().unwrap(),
               &json!({ "value": 42 }));
}

#[tokio::test]
async fn test_ext_notification_unregistered_silent() {
    let client = TestClient::connect().await?;

    // 1. 发送未注册通知（应不返回错误，仅记录日志）
    let result = client.notify("_unregistered", json!({})).await;

    // 2. 通知本身不应产生错误
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ext_notification_no_response_expected() {
    let client = TestClient::connect().await?;

    // 1. 启动监听者
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    client.register_ext_notification("_count", move |_| {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }).await;

    // 2. 连续发送 3 个通知
    for i in 0..3 {
        client.notify("_count", json!({ "i": i })).await?;
    }

    // 3. 等待所有通知处理
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 4. 验证
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}
```

### 验收清单

- [ ] `protocol.rs` 中定义 `EXT_NOTIFICATION_PATTERN` 正则
- [ ] `agent.rs` 中实现 `handle_ext_notification` 函数
- [ ] `agent.rs` 中实现 `register_ext_notification` 公开 API
- [ ] `agent.rs` 中实现 `send_ext_notification` 主动发送 API
- [ ] `stdio_loop.rs` 中在 notification 分派器中添加 `_` 前缀 fallback
- [ ] 验证下划线前缀：非 `_` 开头的通知路由到未知通知
- [ ] 验证未注册通知静默忽略（不返回错误）
- [ ] 验证通知无 `id` 字段、无响应帧
- [ ] 添加 `e2e_mega.rs` 测试用例（3 个：接收、未注册静默、连续发送）
