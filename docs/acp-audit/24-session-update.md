# ACP 协议审计：session/update

## 协议规范

`session/update` 是 Agent-to-Client 通知协议，用于发送 session 更新流式通知。它携带以下几种子类型之一：
- `agent_message_chunk` — agent 输出分块
- `user_message_chunk` — 用户输入回显/分块
- `thought_chunk` — 推理/思考流分块
- `tool_call` — 工具调用开始
- `tool_call_update` — 工具调用进度/结果
- `plan` — 计划/推理步骤更新
- `available_commands_update` — 可用命令列表已更改
- `current_mode_update` — session 模式已更改
- `config_option_update` — 配置选项已更改
- `session_info_update` — session 元数据更新

这是一个**仅通知**通道（Agent → Client）；client 不以 result 负载响应。

---

## 实现状态

**已实现**（10 个子类型中有 9 个在代码中确认；2 个子类型零实现存在）

| 子类型 | 状态 |
|---|---|
| `agent_message_chunk` | ✅ 已实现 |
| `user_message_chunk` | ✅ 已实现 |
| `thought_chunk` | ✅ 已实现 |
| `tool_call` | ✅ 已实现 |
| `tool_call_update` | ✅ 已实现 |
| `plan` | ✅ 已实现 |
| `available_commands_update` | ❌ 未实现 |
| `current_mode_update` | ✅ 已实现 |
| `config_option_update` | ❌ 未实现 |
| `session_info_update` | ✅ 已实现 |

---

## 实现细节

### 核心枚举：`StreamUpdate`

**文件：** `apps/acp/src/protocol.rs`

```rust
pub enum StreamUpdate {
    AgentMessageChunk(...),
    UserMessageChunk(...),
    ThoughtChunk(...),
    ToolCall(...),
    ToolCallUpdate(...),
    Plan(...),
    AvailableCommandsUpdate(...),  // present in enum, no emit sites
    CurrentModeUpdate(...),         // present in enum, dedicated sender
    ConfigOptionUpdate(...),        // present in enum, no emit sites
    SessionInfoUpdate(...),
}
```

枚举涵盖全部 10 个变体。`stream_bridge.rs` 第 17 行的注释 `"plan / available_commands_update / current_mode_update"` 将它们列在同一行，暗示它们共享相同的 Loom 源 — 此注释略有误导性，因为 `current_mode_update` 有自己的专用路径。

---

### 子类型：`current_mode_update`

**文件：** `apps/acp/src/stream_bridge.rs:880–899`

```rust
SessionNotifier::send_current_mode(...)
SessionNotifier::try_send_current_mode(...)
```

**触发点：** `apps/acp/src/agent.rs:333–336` 通过 `apply_session_mode`：

```rust
fn apply_session_mode(...) {
    notifier.send_current_mode(...);
}
```

---

### 子类型：`agent_message_chunk`、`user_message_chunk`、`thought_chunk`

**文件：** `apps/acp/src/stream_bridge.rs`

这些在 agent 运行路径的流式循环期间通过 `SessionNotifier::send_*` / `try_send_*` 方法发出。

---

### 子类型：`tool_call`、`tool_call_update`

**文件：** `apps/acp/src/stream_bridge.rs`

工具调用开始和增量更新变体接入到工具执行管道中。

---

### 子类型：`plan`

**文件：** `apps/acp/src/stream_bridge.rs`

在计划/推理步骤期间发出。

---

### 子类型：`session_info_update`

**文件：** `apps/acp/src/stream_bridge.rs`

在 session 元数据更改时发出。

---

### 差距：`available_commands_update` 和 `config_option_update`

两个变体都存在于 `StreamUpdate` 枚举中，但在代码库中零发出点。在已确认文件中没有任何对 `send_available_commands` 或 `send_config_option` / `try_send_config_option` 的调用：

- `apps/acp/src/stream_bridge.rs`
- `apps/acp/src/protocol.rs`
- `apps/acp/src/agent.rs`
- `apps/acp/src/stdio_loop.rs`
- `apps/acp/tests/e2e_mega.rs`

它们**已声明但从未触发**。

---

## 实现方式

Loom 将 `session/update` 实现为通过 `stream_bridge.rs` 中 `SessionNotifier` 的**广播通道**。Notifier 持有 `tokio::sync::broadcast::Sender<StreamUpdate>`，每个 client 连接持有相应的 `Receiver`。当 agent 通过 notifier 发出 `StreamUpdate` 变体时，所有连接的 client 都会接收到。

`StreamBridge` 充当 agent 内部事件发出与 ACP wire 协议之间的 bridge，将 `StreamUpdate` 变体序列化到通过传输（在 e2e 测试中是 stdio）发送的 ACP 帧中。

---

## 差距与问题

1. **`available_commands_update` — 零发出点。** 变体在 `StreamUpdate` 枚举中已声明，但没有任何代码路径调用 `send_available_commands` 或等效方法。如果规范要求此通知，则目前是静默的无操作。

2. **`config_option_update` — 零发出点。** 同样的情况：在枚举中已声明，从未发出。如果运行时配置更改应传播到 client，则这些代码路径缺失。

3. **`stream_bridge.rs:17` 处的误导性注释。** 该注释将 `plan / available_commands_update / current_mode_update` 归为一组，暗示共享源，但 `current_mode_update` 具有自己专用的 `SessionNotifier::send_current_mode` 发送器，与其他两个不同。

---

## 验证

**对 5 个已确认文件执行对抗性验证：**

- `apps/acp/src/stream_bridge.rs`
- `apps/acp/src/protocol.rs`
- `apps/acp/src/agent.rs`
- `apps/acp/src/stdio_loop.rs`
- `apps/acp/tests/e2e_mega.rs`

**结论：**
- 9 个子类型已确认具有实际的发出点或专用 sender 方法。
- 2 个子类型（`AvailableCommandsUpdate`、`ConfigOptionUpdate`）已确认在枚举声明和文档注释之外**零存在**。
- 未发现缺失变体的替代实现。
- `current_mode_update` 正确追溯到 `stream_bridge.rs:880–899` 处的 `SessionNotifier::send_current_mode` / `try_send_current_mode`，从 `agent.rs:333–336` 处的 `apply_session_mode` 触发。

**结论：已验证确认。** 先前的分析准确。9 个已实现变体已正确定位；2 个差距是真实的。未发现缺失变体的替代实现。

---

## 总结

`session/update` 在 Loom 中**大部分已实现**。ACP `StreamUpdate` 枚举完全列举了全部 10 个子类型，其中 8 个通过 `SessionNotifier` 具有可用的发出路径。两个子类型（`available_commands_update`、`config_option_update`）已声明但从未触发 — 这些应该使用它们缺失的发出点来实现，或者如果规范不要求，则从枚举中删除。

**建议：**
1. 审计 `available_commands_update` 和 `config_option_update` 是否被 ACP 规范所要求。如果是，则实现其发出路径；如果不是，则删除死代码枚举变体以避免混淆。
2. 更正 `stream_bridge.rs:17` 处的注释以准确反映 `current_mode_update` 具有自己专用的 sender，而不是将其与 `plan` 归为一组。

---

## 实现指南

### 当前实现摘要

```rust
// apps/acp/src/stream_bridge.rs:17 (注释误导)
// 注：注释将 plan / available_commands_update / current_mode_update 归为一组
// 实际：current_mode_update 有自己专用的 sender

pub enum StreamUpdate {
    AgentMessageChunk(...),        // ✅
    UserMessageChunk(...),         // ✅
    ThoughtChunk(...),             // ✅
    ToolCall(...),                 // ✅
    ToolCallUpdate(...),           // ✅
    Plan(...),                     // ✅
    AvailableCommandsUpdate(...),  // ❌ 已声明但从未触发
    CurrentModeUpdate(...),        // ✅（专用 sender）
    ConfigOptionUpdate(...),       // ❌ 已声明但从未触发
    SessionInfoUpdate(...),        // ✅
}
```

### 差距 1 修复：实现 available_commands_update 发送点

**问题位置：** `apps/acp/src/stream_bridge.rs` 整个文件（零发送点）

**前置分析：**
- 该子类型在 `StreamUpdate` 枚举中已声明
- `SessionNotifier` 无对应的 `send_available_commands` 或 `try_send_available_commands` 方法
- `agent.rs` 中无任何调用点

**修复 — 添加 SessionNotifier 方法：**

```rust
// apps/acp/src/stream_bridge.rs
impl SessionNotifier {
    // 新增方法
    pub fn send_available_commands(
        &self,
        commands: Vec<AvailableCommand>,
    ) -> Result<(), SendError> {
        let update = StreamUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate {
            available_commands: commands,
            meta: None,
        });
        self.tx.send(update)
    }

    pub fn try_send_available_commands(
        &self,
        commands: Vec<AvailableCommand>,
    ) -> Result<(), TrySendError> {
        let update = StreamUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate {
            available_commands: commands,
            meta: None,
        });
        self.tx.try_send(update)
    }
}
```

**修复 — 在 agent 中触发：**

```rust
// apps/acp/src/agent.rs — 在适当的生命周期事件触发
impl Agent {
    /// 当 session 启动或命令列表变化时，发送可用命令
    pub async fn announce_available_commands(&self) -> Result<(), AgentError> {
        let commands = self.collect_available_commands();
        self.notifier.send_available_commands(commands)?;
        Ok(())
    }

    fn collect_available_commands(&self) -> Vec<AvailableCommand> {
        // 1. 始终可用的内置命令
        let mut commands = vec![
            AvailableCommand {
                name: "reset".to_string(),
                description: "Reset the session".to_string(),
                input_hint: None,
            },
            AvailableCommand {
                name: "review-skill".to_string(),
                description: "Trigger skill review".to_string(),
                input_hint: None,
            },
        ];

        // 2. 模式特定命令
        if self.current_mode == Mode::Dev {
            commands.push(AvailableCommand {
                name: "goal".to_string(),
                description: "Set session goal".to_string(),
                input_hint: Some("<goal text>".to_string()),
            });
        }

        commands
    }
}

// 在 initialize / session/new / mode change 时调用
pub async fn handle_session_new(&self, ...) -> Result<...> {
    // ... 创建 session ...
    self.announce_available_commands().await?;
    Ok(...)
}

pub async fn handle_session_set_mode(&self, ...) -> Result<...> {
    // ... 应用 mode ...
    self.announce_available_commands().await?;  // ← mode 变化时也发送
    Ok(...)
}
```

**设计决策：** 在哪些时刻发送？

| 时机 | 是否发送 | 理由 |
|------|---------|------|
| `session/new` | ✅ | 客户端初始化时需要 |
| `session/load` | ✅ | 加载历史时需要 |
| `session/resume` | ✅ | resume 时需要 |
| `session/set_mode` | ✅ | mode 变化影响可用命令 |
| `session/close` | ❌ | session 已结束，无意义 |
| `session/prompt` 中 | ❌ | 太频繁，会淹没流 |

### 差距 2 修复：实现 config_option_update 发送点

**问题位置：** `apps/acp/src/stream_bridge.rs`

**修复 — 添加 SessionNotifier 方法：**

```rust
impl SessionNotifier {
    pub fn send_config_option_update(
        &self,
        config_id: String,
        new_value: ConfigValue,
    ) -> Result<(), SendError> {
        let update = StreamUpdate::ConfigOptionUpdate(ConfigOptionUpdate {
            config_id,
            value: new_value,
            meta: None,
        });
        self.tx.send(update)
    }

    pub fn try_send_config_option_update(
        &self,
        config_id: String,
        new_value: ConfigValue,
    ) -> Result<(), TrySendError> {
        let update = StreamUpdate::ConfigOptionUpdate(ConfigOptionUpdate {
            config_id,
            value: new_value,
            meta: None,
        });
        self.tx.try_send(update)
    }
}
```

**修复 — 在配置变更处理器中触发：**

```rust
// apps/acp/src/agent.rs
pub async fn handle_session_set_config_option(
    &self,
    req: SessionSetConfigOptionRequest,
) -> Result<SessionSetConfigOptionResponse, AgentError> {
    // ... 验证 + 持久化 ...

    // 持久化成功后通知 client
    self.notifier.send_config_option_update(
        req.config_key.clone(),
        req.config_value.clone(),
    )?;

    Ok(SessionSetConfigOptionResponse {
        config_key: req.config_key,
        // ...
    })
}

// 也要在 mode 变化时通知（mode 是一种 config option）
pub async fn apply_session_mode(&self, new_mode: Mode) -> Result<(), AgentError> {
    // ... 应用 mode ...
    self.notifier.send_config_option_update(
        "mode".to_string(),
        ConfigValue::String(new_mode.to_string()),
    )?;
    Ok(())
}
```

### 差距 3 修复：更正误导性注释

**问题位置：** `apps/acp/src/stream_bridge.rs:17`

**修复前：**
```rust
// stream_bridge.rs:17
// 注：plan / available_commands_update / current_mode_update 共享源
// 实际：注释错误，current_mode_update 有自己专用的 sender
pub enum StreamUpdate {
    // ...
    Plan(...),                     // 来源：streaming bridge
    AvailableCommandsUpdate(...),  // ❌ 没有发送点
    CurrentModeUpdate(...),        // 来源：apply_session_mode 专用 sender
    // ...
}
```

**修复后：**

```rust
// stream_bridge.rs:17（更新注释）
/// StreamUpdate 变体的来源：
///
/// 来自流式 bridge（agent 工具循环）：
///   - AgentMessageChunk
///   - UserMessageChunk
///   - ThoughtChunk
///   - ToolCall
///   - ToolCallUpdate
///   - Plan
///   - SessionInfoUpdate
///
/// 来自专用 sender（在特定生命周期事件触发）：
///   - CurrentModeUpdate       ← SessionNotifier::send_current_mode()
///                                (由 apply_session_mode 触发)
///
/// 已声明但需添加发送点（见 docs/acp-audit/24-session-update.md）：
///   - AvailableCommandsUpdate ← 应在 session/new, session/load, mode change 触发
///   - ConfigOptionUpdate      ← 应在 session/set_config_option, mode change 触发
pub enum StreamUpdate {
    // ...
}
```

### 演示：完整的 session/update 流

**Client 启动 session：**
```json
// 1. Client: session/new
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "session/new",
  "params": { "cwd": "/home/user/proj" }
}

// 2. Agent 响应 + 立即推送 commands
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": { "sessionId": "sess-abc" }
}

// 3. Agent → Client: available_commands_update 通知
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc",
    "update": {
      "sessionUpdate": "available_commands_update",
      "availableCommands": [
        { "name": "reset",       "description": "Reset the session" },
        { "name": "review-skill","description": "Trigger skill review" }
      ]
    }
  }
}
```

**Mode 变化触发 commands + config 更新：**
```json
// 1. Client: session/set_mode (dev → ask)
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "session/set_mode",
  "params": { "modeId": "ask" }
}

// 2. Agent 响应
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": { "modeId": "ask" }
}

// 3. Agent → Client: config_option_update (mode 改变)
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc",
    "update": {
      "sessionUpdate": "config_option_update",
      "configId": "mode",
      "value": "ask"
    }
  }
}

// 4. Agent → Client: current_mode_update（专用 sender）
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc",
    "update": {
      "sessionUpdate": "current_mode_update",
      "currentModeId": "ask"
    }
  }
}

// 5. Agent → Client: available_commands_update (ask 模式没有 goal 命令)
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc",
    "update": {
      "sessionUpdate": "available_commands_update",
      "availableCommands": [
        { "name": "reset",       "description": "Reset the session" },
        { "name": "review-skill","description": "Trigger skill review" }
      ]
    }
  }
}
```

**配置选项变化：**
```json
// 1. Client: session/set_config_option (model)
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "session/set_config_option",
  "params": { "configKey": "model", "configValue": "claude-opus-4-5" }
}

// 2. Agent 响应 + 推送 config_option_update
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": { "configKey": "model" }
}

{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc",
    "update": {
      "sessionUpdate": "config_option_update",
      "configId": "model",
      "value": "claude-opus-4-5"
    }
  }
}
```

### 演示：变体来源矩阵（修复后）

| 变体 | 来源 | 触发点 | 频率 |
|------|------|---------|------|
| `agent_message_chunk` | 流式 bridge | 每次 token | 高 |
| `user_message_chunk` | 流式 bridge | 用户输入时 | 中 |
| `thought_chunk` | 流式 bridge | LLM 推理时 | 中 |
| `tool_call` | 流式 bridge | 工具开始 | 中 |
| `tool_call_update` | 流式 bridge | 工具进度/完成 | 中 |
| `plan` | 流式 bridge | 计划更新 | 低 |
| `available_commands_update` | **新增** send_available_commands | session/new, load, mode change | 低 |
| `current_mode_update` | 专用 send_current_mode | mode 变化 | 低 |
| `config_option_update` | **新增** send_config_option_update | set_config_option, mode change | 低 |
| `session_info_update` | 流式 bridge | 元数据变化 | 低 |

### 测试场景

在 `apps/acp/tests/stream_bridge_tests.rs` 中添加：

```rust
#[tokio::test]
async fn test_available_commands_sent_on_session_new() {
    // 差距 1 修复
    let client = TestClient::connect().await?;
    client.session_new("test").await?;

    // 验证：收到 available_commands_update 通知
    let received = client.wait_for_session_update("available_commands_update").await?;
    let update: AvailableCommandsUpdate = received.update.into();
    assert!(!update.available_commands.is_empty());
    assert!(update.available_commands.iter().any(|c| c.name == "reset"));
}

#[tokio::test]
async fn test_available_commands_sent_on_mode_change() {
    // 差距 1 修复 — 验证 mode 变化触发
    let client = TestClient::connect().await?;
    let session_id = client.session_new("test").await?;

    // 切换到 dev 模式
    client.session_set_mode(session_id.clone(), "dev").await?;

    // 验证：再次收到 available_commands_update（带 goal 命令）
    let received = client.wait_for_session_update("available_commands_update").await?;
    let update: AvailableCommandsUpdate = received.update.into();
    assert!(update.available_commands.iter().any(|c| c.name == "goal"),
            "dev mode should include goal command");
}

#[tokio::test]
async fn test_config_option_update_sent_on_set_config() {
    // 差距 2 修复
    let client = TestClient::connect().await?;
    let session_id = client.session_new("test").await?;

    // 启动监听
    let updates = Arc::new(Mutex::new(Vec::new()));
    let u = updates.clone();
    client.on_session_update(move |update| {
        let u = u.clone();
        async move {
            u.lock().await.push(update);
        }
    }).await;

    // 修改配置
    client.session_set_config_option(session_id, "model", "claude-opus").await?;

    // 验证：收到 config_option_update
    tokio::time::sleep(Duration::from_millis(100)).await;
    let updates = updates.lock().await;
    assert!(updates.iter().any(|u| matches!(u, StreamUpdate::ConfigOptionUpdate(_))));
}

#[tokio::test]
async fn test_config_option_update_sent_on_mode_change() {
    // 差距 2 修复 — mode 是 config option
    let client = TestClient::connect().await?;
    let session_id = client.session_new("test").await?;

    let updates = Arc::new(Mutex::new(Vec::new()));
    let u = updates.clone();
    client.on_session_update(move |update| {
        let u = u.clone();
        async move {
            u.lock().await.push(update);
        }
    }).await;

    client.session_set_mode(session_id, "dev").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let updates = updates.lock().await;
    let config_updates: Vec<_> = updates.iter()
        .filter(|u| matches!(u, StreamUpdate::ConfigOptionUpdate(_)))
        .collect();
    assert!(!config_updates.is_empty(),
            "mode change should trigger config_option_update");
}
```

### 验收清单

**差距 1 — available_commands_update 发送：**
- [ ] `stream_bridge.rs` 添加 `send_available_commands` 方法
- [ ] `stream_bridge.rs` 添加 `try_send_available_commands` 方法
- [ ] `agent.rs` 在 `session/new`, `session/load`, `session/resume` 触发
- [ ] `agent.rs` 在 `apply_session_mode` 后也触发
- [ ] 添加 `collect_available_commands` 辅助函数
- [ ] 添加 2 个测试（session/new + mode change）

**差距 2 — config_option_update 发送：**
- [ ] `stream_bridge.rs` 添加 `send_config_option_update` 方法
- [ ] `stream_bridge.rs` 添加 `try_send_config_option_update` 方法
- [ ] `agent.rs` 在 `handle_session_set_config_option` 触发
- [ ] `agent.rs` 在 `apply_session_mode` 后也触发（mode 是 config option）
- [ ] 添加 2 个测试（set_config + mode change）

**差距 3 — 注释更正：**
- [ ] `stream_bridge.rs:17` 添加准确的"变体来源"文档
- [ ] 明确区分流式 bridge 变体和专用 sender 变体
- [ ] 标记未实现的变体（available_commands_update, config_option_update）

**测试覆盖：**
- [ ] 4 个新测试（命令发送 × 2，配置更新 × 2）
- [ ] 验证修复前失败 / 修复后通过
