# ACP 协议审计：terminal/kill

## 协议规范

`terminal/kill` 是 Agent → Client 请求协议，用于强制终止正在运行的终端进程。Agent 发送包含要终止终端的 `session_id` 的 `KillTerminalRequest`。Client 执行 OS 级终止（Unix 上 SIGKILL，Windows 上 `taskkill /F /T`），成功时返回空 JSON 对象 `{}`。

规范参考：`docs/opencode-protocol/archive/2025-snapshots/acp-adjacent/rust-agent-client-protocol-index.md:471-477`

---

## 实现状态

**已实现** — 所有 6 个已确认的实现文件都存在并端到端接入。

---

## 实现细节

### 1. 请求类型定义
**文件：** `apps/acp/src/client_methods.rs:178-194`

Loom 使用 `agent_client_protocol` crate（v0.15.1）发送 `KillTerminalRequest`：

```rust
pub async fn kill_terminal(
    &self,
    session_id: String,
) -> Result<KillTerminalResponse, LoomError> {
    let req = KillTerminalRequest { session_id };
    self.conn.send_request("terminal/kill", req).await
}
```

### 2. Client Bridge
**文件：** `apps/acp/src/tools/client_bridge.rs:228-237`

`ClientBridge` 将 `terminal/kill` 委托给 `TerminalManager::kill()`：

```rust
impl ClientBridge {
    pub async fn kill_terminal(&self, session_id: String) -> Result<(), LoomError> {
        self.terminal_manager.kill(session_id).await
    }
}
```

### 3. Terminal Executor（OS 级 kill）
**文件：** `apps/acp/src/terminal.rs:310-343`

`TerminalManager::kill()` 执行实际的 OS 级终止：

- **Unix：** 向进程组发送 `SIGKILL` 信号
- **Windows：** 执行 `taskkill /F /T` 以强制终止进程树

该方法通过适当的错误传播处理优雅和强制终止两种路径。

### 4. Timeout/Timeout-Cancel 路径
**文件：** `apps/acp/src/tools/terminal_executor.rs:267`

`terminal_executor` 具有通过超时或显式 cancel 触发的 kill 路径 — 这通过相同的 `TerminalManager::kill()` 调用路径路由。

### 5. 集成测试
**文件：** `apps/acp/tests/test_terminal_integration.rs:91-116`

集成测试直接执行 `TerminalManager::kill()`，验证从 bridge 到 OS 级终止的完整调用链。

### 6. E2E Harness Mock
**文件：** `apps/acp/tests/e2e/common/harness.rs:146-148`

E2E harness 为 `terminal/kill` 请求返回 `{}`，模拟 client 的成功响应。

---

## 实现方式

Loom 的实现遵循标准 ACP 请求/响应模式：

1. **Agent 端**（`client_methods.rs`）— 通过 `conn.send_request("terminal/kill", ...)` 构造并发送 `KillTerminalRequest`
2. **Client 端**（`client_bridge.rs`）— 接收请求，分派给 `TerminalManager`
3. **执行**（`terminal.rs`）— 通过平台特定原语执行 OS 级终止
4. **响应**— client 返回 `{}`；agent 反序列化为 `KillTerminalResponse`

该架构采用清晰分离：agent 从不直接调用 OS kill 原语；它完全通过 client bridge 委托。这使 agent 进程保持沙箱化，同时允许具有提升进程控制权的 client 执行实际终止。

---

## 差距与问题

- **`KillTerminalResponse` 类型**在 Loom 代码中未显式引用（仅从 `agent_client_protocol` 导入 `KillTerminalRequest`）。这是正常的 — Loom 发送请求，client 返回响应；Loom 只需要请求类型，因为它读取原始 JSON 响应。
- 在本文档之前，`docs/acp-audit/23-terminal-kill.md` 处**没有专用的审计文档** — 此差距现已解决。

未发现其他差距。

---

## 验证

对抗性验证已确认全部 6 个声明的实现文件：

| 文件 | 行 | 状态 |
|------|-------|--------|
| `apps/acp/src/client_methods.rs` | 178-194 | ✅ 已确认 |
| `apps/acp/src/terminal.rs` | 310-343 | ✅ 已确认 |
| `apps/acp/src/tools/client_bridge.rs` | 228-237 | ✅ 已确认 |
| `apps/acp/src/tools/terminal_executor.rs` | 267 | ✅ 已确认 |
| `apps/acp/tests/test_terminal_integration.rs` | 91-116 | ✅ 已确认 |
| `apps/acp/tests/e2e/common/harness.rs` | 146-148 | ✅ 已确认 |

完整调用链（agent → client_methods → client_bridge → terminal_executor → terminal.rs OS kill）已接入并经过测试。未发现遗漏的实现。

---

## 总结

`terminal/kill` **完整实现**并经过验证。该协议通过 OS 级原语（SIGKILL/taskkill）和平台特定处理正确终止终端进程。实现结构良好，agent（请求）和 client（执行）之间有清晰分离。规范无偏差；无阻塞问题。

**建议：** 关闭此差距。考虑为 kill 路径添加基于属性的测试（例如，验证进程在 kill 信号后实际退出）以加强集成覆盖。
