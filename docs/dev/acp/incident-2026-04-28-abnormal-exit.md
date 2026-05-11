# 事故报告：loom_acp 异常退出 — 2026-04-28

> 日期：2026-04-28
> 日志源：`logs/acp.2026-04-28.log`（178,018 行）

## 概述

loom_acp 进程在 2026-04-28 期间共发生 **2 次异常退出**，均由工具超时触发级联 `receiver dropped` 错误导致进程终止。另有 1 次超时未导致退出（session `ef9c74d3`）。

## 时间线

| 时间 (UTC) | Session | Terminal ID | 事件 | 结果 |
|------------|---------|-------------|------|------|
| 01:48:04 | `29f368a8` | `0f453a12` | bash 命令超时 120s，被 kill | 进程退出 |
| 02:39:55 | `29f368a8` | `8431ecb8` | bash 命令超时 120s，被 kill + `connect_to failed` | 进程退出 |
| 02:40:43 | — | — | Zed 自动重连，`initialize` 成功 | 恢复 |
| 03:54:56 | `ef9c74d3` | `3b19bf30` | bash 命令超时 120s，被 kill | 未退出 |

## 错误链路

```
bash 工具执行超过 120s
    ↓
terminal_executor 触发超时 → WARN: command timed out, killing
    ↓
terminal/kill 请求发送到 IDE 侧
    ↓
IDE (Zed) 关闭连接 / stdin EOF
    ↓
JSON-RPC transport actor 关闭 → outgoing mpsc channel receiver 被 drop
    ↓
正在运行的 prompt task 通过 Responder 发送响应
    ↓
"failed to send response, receiver dropped"  ← ERROR 级别
    ↓
connect_to 返回 Err → run_until 退出 → LocalSet 销毁
    ↓
INFO: run_stdio_loop finished (connection closed)
INFO: loom-acp exiting normally  ← 误导性日志
```

## 日志证据

### 第一次退出 (01:48:04)

```
WARN  loom_acp::tools::terminal_executor: command timed out, killing
      terminal_id=0f453a12 timeout_ms=120000
DEBUG loom_acp::client_methods: Sending terminal/kill request
```

### 第二次退出 (02:39:55-02:39:56)

```
02:39:55.998 WARN  loom_acp::tools::terminal_executor: command timed out, killing
                terminal_id=8431ecb8 timeout_ms=120000
02:39:56.040 DEBUG loom::agent::react::think_node: think: invoking LLM messages=122
02:39:56.056 ERROR loom_acp: connect_to failed
                e=Error { code: -32603: "failed to send response, receiver dropped" }
02:39:56.066 INFO  loom_acp: run_stdio_loop finished (connection closed)
02:39:56.066 INFO  loom_acp: loom-acp exiting normally
```

47 秒后 Zed 自动重连：

```
02:40:43.281 INFO  loom_acp: run_stdio_loop starting
02:40:43.290 TRACE ... Received JSON-RPC message {"method":"initialize","params":{...,"clientInfo":{"name":"zed"}}}
02:40:43.292 INFO  loom_acp::agent: initialize called protocol_version=ProtocolVersion(1)
```

## 关键发现

1. **`exiting normally` 是误导性日志** — 进程因 `connect_to` 错误退出，并非正常结束。应区分 "连接正常关闭" 和 "运行中连接断开"。

2. **超时阈值** — 所有超时均为 120,000ms (120s)，由 `terminal_executor` 的 `timeout_ms` 参数控制。

3. **超时未必定导致退出** — session `ef9c74d3` 的超时 (03:54:56) 没有触发 `connect_to failed`，说明是否退出取决于超时发生时 IDE 连接是否同时断开。

4. **Zed 自动重连** — 进程退出后约 47s，Zed (v0.234.6) 自动发起 `initialize` 重连成功。

5. **Session 复用** — session `29f368a8` 在两次退出间被复用，说明 IDE 侧重连后恢复了原 session。

## 影响

- Agent 正在执行的任务被中断（LLM 调用、工具执行）
- 已消耗的 LLM token 浪费（think_node 刚发出请求即被中断）
- 用户体验中断，需等待自动重连

## 修复建议

详见 `loom-acp/docs/ERROR_RECEIVER_DROPPED.md` 中的修复方案。关键优先级：

1. **[短期]** 区分 "receiver dropped" 和真正的内部错误，降级日志为 INFO
2. **[短期]** 修正 `exiting normally` 日志，连接断开时应打印不同信息
3. **[中期]** 集成 CancellationToken，连接断开时主动取消 agent 而非等待错误传播
4. **[长期]** 连接健康检查机制，避免在死连接上继续消耗 LLM token

## 相关文件

| 文件 | 说明 |
|------|------|
| `loom-acp/docs/ERROR_RECEIVER_DROPPED.md` | 根因分析和修复方案（已有） |
| `loom-acp/src/lib.rs:448-463` | `connect_to` 错误处理 |
| `loom-acp/src/tools/terminal_executor.rs:66-81` | 工具超时处理 |
| `logs/acp.2026-04-28.log:103922` | 第二次退出的 ERROR 日志 |
