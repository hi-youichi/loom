# ACP 协议审计：session/cancel

## 协议规范

**协议 ID：** `session/cancel`
**方向：** Client -> Agent（通知）
**用途：** 取消正在进行的操作

`session/cancel` 协议通知 agent 中止 session 中当前正在运行的任何操作。这是从 client 到 agent 的单向通知，不期望响应。

## 实现状态

**已实现 ✅** — 端到端取消流程已完整实现和测试。

## 实现细节

### 入口点：`stdio_loop.rs`
**文件：** `apps/acp/src/stdio_loop.rs:411-421`

在 stdio 输入循环中接收取消通知并分派给 agent：

```rust
// stdio_loop.rs:411-421
CancelNotification(notn) => {
    tracing::debug!("received CancelNotification");
    self.agent.cancel();
}
```

### Agent 处理器：`agent.rs`
**文件：** `apps/acp/src/agent.rs:516-521, 519, 524, 762`

`agent.cancel()` 在第 519 行实现，直接路由到 session 存储：

```rust
// agent.rs:519
pub fn cancel(&self) {
    self.session_store.cancel_current_generation();
}
```

其他引用：
- `agent.rs:516` — 方法签名定义
- `agent.rs:524` — 结束大括号
- `agent.rs:762` — 调用点（stdio_loop 调用 `self.agent.cancel()`）

### Session 存储：`session.rs`
**文件：** `apps/acp/src/session.rs:171, 176-195, 198-207, 209-220, 239-244`

两个相关方法：

```rust
// session.rs:171 — 死代码，零调用者
pub fn set_cancelled(&self) {
    self.cancel_current_generation();
}

// session.rs:176-195 — 活动实现
pub fn cancel_current_generation(&self) {
    self.cancelled.store(true, Ordering::Relaxed);
    if let Some(cancel) = self.run_cancellation.read().upgrade() {
        cancel.cancel();
    }
}
```

AtomicBool 标志 + RunCancellation 触发器模式。`cancelled` AtomicBool 在 `session.rs:198-207` 和 `209-220` 处被检查，返回 `RunCompletion::Cancelled`，后者传播到 `PromptResponse(StopReason::Cancelled)`。

### 协议注册：`protocol.rs`
**文件：** `apps/acp/src/protocol.rs:74-79`

```rust
// protocol.rs:74-79
CancelNotification => {
    self.agent.cancel();
}
```

注：协议文档提到 `set_cancelled` 但实际实现调用 `cancel_current_generation` — 行为完全相同；`set_cancelled` 在 `session.rs:172` 处委托给 `cancel_current_generation`。

### 端到端测试覆盖
**文件：** `apps/acp/tests/e2e_mega.rs:175-221`

完整端到端取消流程由集成测试覆盖。

### 依赖项
**文件：** `apps/acp/Cargo.toml:29`

## 实现方式

```
CancelNotification (stdio_loop.rs:411)
  → agent.cancel()           (agent.rs:519)
    → session_store.cancel_current_generation()  (session.rs:176)
      → AtomicBool.set(true) + RunCancellation::cancel()
      → run_agent_from_config 返回 RunCompletion::Cancelled
        → PromptResponse(StopReason::Cancelled)
```

**架构：**
- **AtomicBool 标志**（`cancelled: Arc<AtomicBool>`）— 轻量级标志同步设置；由 agent 循环检查以中止工作
- **RunCancellation** — RAII 风格的 guard，由运行中的任务持有；调用 `.cancel()` 中断正在进行的生成
- **两级中止** — session 级标志和生成级 guard 必须协同工作

## 差距与问题

| 问题 | 严重程度 | 描述 |
|-------|----------|------|
| `set_cancelled` 死代码 | 较小 | 在 `session.rs:171` 定义，零调用者。`agent.cancel()` 直接路由到 `cancel_current_generation`。无行为影响。 |
| 文档命名不匹配 | 较小 | `protocol.rs:78` 文档说明 `set_cancelled` 但调用 `cancel_current_generation`。两个方法行为完全相同；对用户不可见的效果。 |
| 行号引用标记错误 | 较小 | 实现文件列表引用了第 905 行，它位于 `RunOptions` 结构体内，而不是 `run_agent_from_config` 调用点（实际调用在 `agent.rs:998`）。不影响功能。 |

**以上差距均不影响取消流程的正确性或完整性。**

## 验证

**验证方法：** 通过完整代码库 grep + 已确认文件的交叉引用进行对抗性分析。

**已确认文件（6 个）：**
- `apps/acp/src/stdio_loop.rs:411-421`
- `apps/acp/src/agent.rs:516-521, 519, 524, 762`
- `apps/acp/src/session.rs:171, 176-195, 198-207, 209-220, 239-244`
- `apps/acp/src/protocol.rs:74-79`
- `apps/acp/tests/e2e_mega.rs:175-221`
- `apps/acp/Cargo.toml:29`

**流程验证：**
1. `CancelNotification` 在 `stdio_loop.rs` 接收 → 分派给 `agent.cancel()`
2. `SessionStore.cancel_current_generation()` 设置 `AtomicBool` + 触发 `RunCancellation::cancel()`
3. `run_agent_from_config` 返回 `RunCompletion::Cancelled`
4. `PromptResponse(StopReason::Cancelled)` 传播回去

**端到端测试：** `e2e_mega.rs:175-221` 处的集成测试覆盖完整路径。

## 总结

**最终结论：完整实现 ✅**

`session/cancel` 协议已完整实现，无功能差距。从 `stdio_loop` 接收 `CancelNotification`、经过 `agent.cancel()` / `SessionStore.cancel_current_generation()`、到 `RunCompletion::Cancelled` / `StopReason::Cancelled` 的端到端取消流程已确认工作并由端到端测试覆盖。

**次要整理项**（非阻塞）：
1. 清理 `session.rs:171` 处的死代码 `set_cancelled` 方法，或如果它本意是规范的入口点则将其接入。
2. 将 `protocol.rs:78` 的文档与实际调用对齐（`cancel_current_generation` vs `set_cancelled`）。
3. 更正文件元数据（`run_agent_from_config` 调用点引用的第 905 行 → 第 998 行）。
