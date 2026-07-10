# ACP 协议审计：terminal/wait_for_exit

## 协议规范

`terminal/wait_for_exit` 协议是 **Client → Agent Request**，使 client 能够阻塞并等待先前通过 `terminal/create` 创建的终端进程退出。请求指定 `terminal_id` 和可选的 `timeout`。Agent 返回包含退出码、信号状态以及进程是否仍在运行的响应。

**关键字段：**
- `terminal_id: String` — 来自 `terminal/create` 的 ID
- `timeout: Option<Duration>` — 可选等待超时

**响应：**
- `exit_code: Option<i32>` — 退出码（如果已退出）
- `signal: Option<String>` — 终止信号（如果被信号杀死）
- `is_running: bool` — 进程是否仍在运行

## 实现状态

**已实现** — 协议完整实现，由 e2e 测试覆盖。

## 实现细节

### 协议注册

**文件：** `apps/acp/src/protocol.rs:24-29` — `ProtocolMethod::TerminalWaitForExit` 变体

### 处理器：`stdio_loop.rs`

**文件：** `apps/acp/src/stdio_loop.rs:423-432` — 路由器注册

```rust
// stdio_loop.rs:423-432
.on_receive_request(
    move |req: WaitForExitRequest, ...| {
        // Route to handle_terminal_wait_for_exit
    },
    ...
)
```

### Agent 处理器：`agent.rs`

**文件：** `apps/acp/src/agent.rs:1454-1493` — `handle_terminal_wait_for_exit`

```rust
// agent.rs:1454-1493
pub async fn handle_terminal_wait_for_exit(
    &self,
    req: WaitForExitRequest,
) -> Result<WaitForExitResponse, AgentError> {
    // Lookup terminal by ID, await exit, return exit_code/signal
}
```

### 终端管理

**文件：** `apps/acp/src/terminal_manager.rs:78-110` — `TerminalManager::wait_for_exit()`

### 端到端测试

**文件：** `apps/acp/tests/e2e_mega.rs:374-410` — `test_terminal_wait_for_exit`

## 实现方式

```
WaitForExitRequest { terminal_id, timeout }
  → agent.handle_terminal_wait_for_exit()
    → TerminalManager::wait_for_exit(terminal_id, timeout)
      → tokio::time::timeout(duration, child.wait())
      → ExitStatus { code, signal }
    → 返回 WaitForExitResponse { exit_code, signal, is_running }
```

**关键实现细节：**
- 使用 `tokio::time::timeout` 进行可配置超时
- `Child::wait()` 异步等待进程退出
- `ExitStatus` 提取退出码或信号
- 超时后返回 `is_running: true`

## 差距与问题

未发现重大差距。实现正确：
- 进程等待
- 超时处理
- 退出码和信号提取
- ID 查找

## 验证

**结论：完整实现** — e2e 测试 `e2e_mega.rs:374-410` 验证完整流程。

## 总结

`terminal/wait_for_exit` 协议**完整实现**。进程等待、超时处理和退出码提取工作正常。
