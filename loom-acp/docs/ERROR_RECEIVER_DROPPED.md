# 错误排查：Tool Call Timeout 导致 "receiver dropped" 级联错误

> 2025-08-19

## 现象

```
2026-04-26T11:35:42.669887Z WARN  loom::agent::react::act_node: Tool call failed tool=bash error=MCP/transport error: Command timed out
2026-04-26T11:35:42.670118Z ERROR loom_acp: run_stdio_loop error e=Error { code: -32603: Internal error, message: "Internal error", data: Some(String("Error { code: -32603: Internal error, message: \"Internal error\", data: Some(String(\"failed to send response, receiver dropped\")) }")) }
2026-04-26T11:35:42.670124Z INFO  loom_acp: run_stdio_loop finished
```

三条日志按时间顺序依次出现，最终 `run_stdio_loop` 终止。

## 根因分析

### 错误链路

```
bash 工具超时 (act_node)           ← 只是一个 warn，agent 可继续运行
       ↓ (并发)
IDE/传输层断开 (stdin EOF / 连接关闭)
       ↓
ACP 协议层 actor 关闭 (incoming + outgoing)
       ↓
outgoing mpsc channel 被 drop
       ↓
run_until 检测到 background 出错 → 返回 Err
       ↓
LocalSet 销毁 → conn.spawn() 的 prompt task 被 cancel ← agent 中断
       ↓
responder.respond_with_result() 发送失败 (channel 已 drop)
       ↓
"failed to send response, receiver dropped" ← jsonrpc.rs:2077
       ↓
错误传播到 run_stdio_loop → 进程退出
```

### 关键结论

**bash 超时本身不是崩溃的直接原因。** 它只是一个并发事件（warn 级别日志），agent 本可以继续运行。

真正的问题是：在 prompt 任务长时间运行期间，**传输层（stdio 连接）被关闭**，导致 ACP 协议层的所有 actor 退出。当 prompt 任务最终尝试通过 `Responder` 发送响应时，底层的 `mpsc::UnboundedSender<OutgoingMessage>` channel 已经被 drop，触发 "receiver dropped" 错误。

**此错误会导致 agent 中断。** `run_stdio_loop` 因错误退出后，`LocalSet` 被销毁，其上通过 `conn.spawn()` 启动的 prompt task 被取消，`agent.prompt()` → `run_agent_with_options()` 中止执行。完整中断链路：

```
连接关闭 → protocol actor 退出 → run_until 返回 Err
→ local.run_until() 结束 → LocalSet drop → prompt task 被 cancel → agent 中断
```

### 代码路径详解

#### 1. Prompt 处理 — `loom-acp/src/lib.rs:356-368`

```rust
.on_receive_request(
    move |req: PromptRequest, responder: Responder<PromptResponse>, conn: ConnectionTo<Client>| {
        let agent = agent3.clone();
        let _ = conn.spawn(async move {
            let result = agent.prompt(req).await;
            // ignore "receiver dropped" errors - connection may have closed
            let _ = responder.respond_with_result(result);
            Ok(())
        });
        async { Ok(()) }
    },
)
```

- Prompt 请求通过 `conn.spawn()` 在独立 task 中执行，不阻塞 I/O 循环。
- handler 立即返回 `Ok(())` 解除 IO loop 阻塞。
- `let _ = responder.respond_with_result(result)` **已经尝试忽略** receiver dropped 错误。

#### 2. Responder 发送 — ACP crate `jsonrpc.rs:1919-1931`

```rust
fn new(message_tx: OutgoingMessageTx, method: String, id: jsonrpcmsg::Id) -> Self {
    Self {
        send_fn: Box::new(move |response| {
            send_raw_message(&message_tx, OutgoingMessage::Response { id, response })
        }),
    }
}
```

`send_raw_message` 向 `mpsc::UnboundedSender<OutgoingMessage>` 发送。当 outgoing_protocol_actor 退出后，receiver 被 drop，`unbounded_send` 返回 `Err`，被包装成 "failed to send response, receiver dropped" 错误。

#### 3. 传播路径 — ACP crate `util.rs:124-141`

```rust
pub async fn run_until<T, E>(background, foreground) -> Result<T, E> {
    match select(pin!(background), pin!(foreground)).await {
        Either::Left((bg_result, fg_future)) => {
            bg_result?;
            fg_future.await
        }
        Either::Right((fg_result, _bg_future)) => {
            fg_result  // foreground 完成，drop background
        }
    }
}
```

`connect_to` 使用 `run_until`，background 是协议层 actor，foreground 是 `pending()`（永远不完成）。当 background（协议 actor）因连接关闭而出错时，`run_until` 返回该错误。

#### 4. Bash 超时 — `loom-acp/src/tools/terminal_executor.rs:66-81`

```rust
tokio::select! {
    status = self.terminal_mgr.wait_for_exit(&terminal_id) => { ... }
    _ = tokio::time::sleep(Duration::from_millis(timeout)) => {
        warn!(...);
        self.terminal_mgr.kill(&terminal_id).await.ok();
        Err(ToolSourceError::Transport("Command timed out".into()))
    }
}
```

超时返回 `ToolSourceError::Transport`，在 `act_node.rs:327-355` 中被捕获为 warn 并转为 `ToolResult`（带 `is_error: true`），agent 可继续推理。**这不会导致崩溃。**

### 触发场景

此错误最可能发生在以下场景：

| 场景 | 触发方式 |
|------|---------|
| IDE 关闭/断开 | 用户关闭 IDE 窗口，stdin EOF |
| IDE 刷新连接 | IDE 重新连接导致旧连接断开 |
| 网络中断 | 远程开发场景下连接丢失 |
| 进程被 kill | 系统或用户强制终止 IDE 端 |
| Agent 长时间运行 | agent 正在执行长时间任务时 IDE 断开 |

## 修复建议

### 短期（抑制噪音日志）

当前 `let _ = responder.respond_with_result(result)` 已经忽略了 responder 错误，但 `run_stdio_loop` 的 `map_err` 仍然会记录 ERROR 级别日志。可以区分 "连接关闭" 和 "真正的内部错误"：

```rust
// lib.rs:456-461
let result = local.run_until(async { ... }).await;

match result {
    Ok(v) => Ok(v),
    Err(e) => {
        let err_str = e.to_string();
        if err_str.contains("receiver dropped") || err_str.contains("failed to send response") {
            tracing::info!("Connection closed while agent was running");
            Ok(())
        } else {
            tracing::error!(?e, "run_stdio_loop error");
            Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        }
    }
}
```

### 中期（优雅关闭）

1. **监听连接关闭信号**：在 prompt 执行期间检测连接状态，提前终止 agent 运行。
2. **CancellationToken 集成**：将 ACP 连接生命周期与 `RunOptions::cancellation` 关联，连接断开时自动取消 agent。
3. **超时后清理**：bash 超时时确保 terminal 资源被正确释放（当前已有 `kill` + `release`）。

### 长期（架构改进）

- 将 `run_until` 的 `pending()` foreground 改为可取消的 future，使 prompt 任务完成时能主动关闭连接。
- 在 agent 执行层增加 "连接健康检查" 机制，避免在死连接上继续消耗 LLM token。

## 相关文件

| 文件 | 职责 |
|------|------|
| `loom-acp/src/lib.rs:356-368` | Prompt handler，spawn prompt task |
| `loom-acp/src/lib.rs:448-463` | connect_to 错误处理与 run_stdio_loop 退出 |
| `loom-acp/src/agent.rs:600-751` | Agent::prompt，调用 run_agent_with_options |
| `loom-acp/src/tools/terminal_executor.rs:66-81` | TerminalCommandExecutor 超时处理 |
| `loom-acp/src/tools/terminal_executor.rs:196-216` | AcpBridgeCommandExecutor 超时处理 |
| `loom/src/agent/react/act_node.rs:327-355` | 工具调用失败处理 |
| ACP crate `jsonrpc.rs:2077` | "receiver dropped" 错误产生点 |
| ACP crate `jsonrpc.rs:1919-1931` | Responder::new，发送逻辑 |
| ACP crate `util.rs:124-141` | run_until，foreground/background 调度 |
