# invoke_agent 子代理 Stream Event 未发送到 ACP — 问题分析

## 现象

当主代理通过 `invoke_agent` 调用子代理时，子代理自身的流事件（LLM 回复、工具调用等）能正常到达 ACP 客户端；但如果子代理内部再次嵌套调用 `invoke_agent`（第 2 层及更深），这些深层子代理的事件会完全丢失，客户端收不到任何更新。

## 根因

`invoke_agent.rs` 中所有调用 `stream_with_config` 的地方，第 4 个参数 `any_stream_event_sender` 都硬编码为 `None`。

这导致子代理内部的 `RunContext.any_stream_event_sender` 为 `None`，进而 `ActNode` 构建的 `ToolCallContext.any_stream_event_sender` 也为 `None`。当子代理内部的 `invoke_agent` 尝试从 `ToolCallContext` 提取 `any_stream_event_sender` 来转发事件时，得到的是 `None`，事件链断裂。

## 事件转发链路

以下是完整的传播路径及断裂点：

```
┌─────────────────────────────────────────────────────────────────────────┐
│ ACP 层 (loom-acp/src/agent.rs:514-520)                                  │
│                                                                         │
│ 创建 any_stream_event_sender ──► 设置到 RunOptions                      │
└──────────────────────────┬──────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ run_agent (loom/src/cli_run/agent.rs:364)                               │
│                                                                         │
│ opts.any_stream_event_sender.clone() 传给 stream_with_config           │
└──────────────────────────┬──────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ compiled.stream (loom/src/graph/compiled.rs:508)                        │
│                                                                         │
│ run_ctx.any_stream_event_sender = any_stream_event_sender  ✔ 有值       │
└──────────────────────────┬──────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ ActNode (loom/src/agent/react/act_node.rs:521)                          │
│                                                                         │
│ tool_ctx.any_stream_event_sender = run_ctx.any_stream_event_sender ✔   │
└──────────────────────────┬──────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ invoke_agent — call_single_exec (invoke_agent.rs:260-267)               │
│                                                                         │
│ on_event = ctx.any_stream_event_sender.clone()  ✔ 从 ToolCallContext   │
│                                                                         │
│ runner.stream_with_config(task, None, on_event, None)                   │
│                                                  ^^^^                    │
│                                              ❌ 第 4 参数为 None!        │
└──────────────────────────┬──────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ 第 1 层子代理运行                                                        │
│                                                                         │
│ • on_event 闭包转发事件给 ACP ✔（主代理的 any_sender 仍有值）            │
│ • run_ctx.any_stream_event_sender = None  ❌（第 4 参数传了 None）       │
│ • tool_ctx.any_stream_event_sender = None  ❌                           │
└──────────────────────────┬──────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────────┐
│ 第 2 层嵌套 invoke_agent                                                │
│                                                                         │
│ on_event = ctx.any_stream_event_sender  → None                         │
│           .map(|sender| { ... })  → None                               │
│                                                                         │
│ runner.stream_with_config(task, None, None, None)                       │
│                                              ^^^^                       │
│                                          ❌ 事件完全丢失！               │
└─────────────────────────────────────────────────────────────────────────┘
```

## 影响范围

| 层级 | 事件是否到达 ACP | 原因 |
|------|-----------------|------|
| 主代理 | ✔ 正常 | ACP 层直接设置了 `any_stream_event_sender` |
| 第 1 层子代理 | ✔ 正常 | `on_event` 闭包从主代理的 `any_stream_event_sender` 创建，能转发 |
| 第 2 层子代理 | ❌ 丢失 | 第 1 层子代理的 `run_ctx.any_stream_event_sender` 为 `None`，无法创建 `on_event` |
| 第 3 层子代理 | ❌ 丢失 | 同上，链路断裂后无法恢复 |

## 修复方案

### 修改文件：`loom/src/tools/invoke_agent.rs`

### 修复点 1：`call_single_exec`（行 260-267）

```rust
// ── 修改前 ──
let outcome = runner
    .stream_with_config(task, None, on_event, None)
    .await
    .map_err(|e| {
        ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
    })?;

// ── 修改后 ──
let any_sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
let outcome = runner
    .stream_with_config(task, None, on_event, any_sender)
    .await
    .map_err(|e| {
        ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
    })?;
```

### 修复点 2：`invoke_single_agent`（行 528-535）

```rust
// ── 修改前 ──
let outcome = runner
    .stream_with_config(task, None, on_event, None)
    .await
    .map_err(|e| {
        ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
    })?;

// ── 修改后 ──
let any_sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
let outcome = runner
    .stream_with_config(task, None, on_event, any_sender)
    .await
    .map_err(|e| {
        ToolSourceError::Transport(format!("sub-agent '{}' failed: {}", agent_name, e))
    })?;
```

## 为什么不会导致双重发送？

`on_event` 和 `any_stream_event_sender` 职责不同，不会产生重复事件：

| 机制 | 触发方式 | 作用 |
|------|---------|------|
| `on_event` 闭包 | `run_stream_with_config` 中的 `while let` 循环 | 接收子代理图的每个 `StreamEvent`，包装为 `AnyStreamEvent::React` 发送给 ACP |
| `any_stream_event_sender` | 存储在 `RunContext` 上，不主动调用 | 仅供嵌套 `invoke_agent` 通过 `ToolCallContext` 提取并继续向下传播 |

图的 `while let` 循环只调用 `on_event`，不会调用 `any_stream_event_sender`。`any_stream_event_sender` 只在被 `ActNode` 提取到 `ToolCallContext` 后、再被嵌套的 `invoke_agent` 使用时才产生效果。两条路径互不交叉。

## 相关文件索引

| 文件 | 关键行号 | 说明 |
|------|---------|------|
| `loom/src/tools/invoke_agent.rs` | 260-267 | **Bug 点 1**：`call_single_exec` 中第 4 参数为 `None` |
| `loom/src/tools/invoke_agent.rs` | 528-535 | **Bug 点 2**：`invoke_single_agent` 中第 4 参数为 `None` |
| `loom/src/tools/invoke_agent.rs` | 254-264 | `on_event` 闭包从 `ctx.any_stream_event_sender` 创建 |
| `loom/src/tools/invoke_agent.rs` | 522-532 | 同上，独立函数版本 |
| `loom/src/tool_source/context.rs` | 84-88 | `ToolCallContext.any_stream_event_sender` 字段定义 |
| `loom/src/agent/react/act_node.rs` | 514-522 | `ActNode` 从 `run_ctx` 传播 `any_stream_event_sender` 到 `ToolCallContext` |
| `loom/src/graph/compiled.rs` | 492-519 | `compiled.stream` 设置 `run_ctx.any_stream_event_sender` |
| `loom/src/graph/run_context.rs` | 107-108 | `RunContext.any_stream_event_sender` 字段定义 |
| `loom/src/runner_common.rs` | 83-136 | `run_stream_with_config` 中的事件循环 |
| `loom/src/agent/react/runner/runner.rs` | 178-212 | `stream_with_config` 签名及透传 |
| `loom-acp/src/agent.rs` | 514-544 | ACP 层创建 `any_stream_event_sender` |
| `loom-acp/src/stream_bridge.rs` | 409-416 | `try_send_event` 转发到 ACP 通道 |
| `loom-acp/src/stream_bridge.rs` | 119-132 | `loom_event_to_updates` 事件转换 |
| `serve/src/run/stream.rs` | 173-197 | Serve 层创建 `any_stream_event_sender` 回调 |
