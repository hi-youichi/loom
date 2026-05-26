# Loom 工具执行状态显示控制指南

## 概述

Loom 在执行工具时会向用户展示中间状态（如工具名和描述）。本文档说明各场景下的显示机制与隐藏方案。

---

## 架构总览

```
Loom Agent (ReAct Loop)
  │
  ├─ StreamEvent::ToolCall   ── 工具调用开始（含 name, arguments）
  ├─ StreamEvent::ToolStart  ── 工具实际执行开始
  ├─ StreamEvent::ToolOutput ── 工具输出增量
  ├─ StreamEvent::ToolEnd    ── 工具执行结束（含 result）
  │
  └─ on_event 回调分发到三个消费者：
       ├─ CLI (cli/src/run/agent.rs)       → spinner 显示 "{name} - {description}"
       ├─ Telegram Bot (telegram-bot)       → event_mapper.rs（已 no-op）
       └─ Web (loom-acp stream_bridge.rs)   → ToolCallStarted → 前端 ToolBlock
```

---

## 1. CLI 终端

### 显示位置

`cli/src/run/agent.rs` 第 433、574 行：

```rust
if let Some(tc) = state.tool_calls.first() {
    sp.update(format!("Executing tool: {}", tc.name));
}
```

当前方案改为不显示 "Executing"，直接显示工具名 + description：

### 方案：spinner 显示工具名 + description

如果工具调用参数中包含 `description` 字段，则 spinner 显示 `"{name} - {description}"`，否则仅显示 `"{name}"`。

```rust
// 修改 cli/src/run/agent.rs
if let Some(tc) = state.tool_calls.first() {
    let desc = tc.arguments.get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let label = if desc.is_empty() {
        tc.name.clone()
    } else {
        format!("{} - {}", tc.name, desc)
    };
    sp.update(label);
}
```

### 含 description 参数的工具

| 工具 | description 含义 | 源码位置 |
|------|-----------------|----------|
| `bash` | 命令用途简述 | `loom/tools/bash.yaml:34` |
| `task_create` | 任务描述 | `loom/src/tools/task/create.rs:36` |
| `task_update` | 任务描述 | `loom/src/tools/task/update.rs:37` |
| `remember` | 记忆内容 | `loom/src/tools/memory/remember.rs:92` |

> 其余工具的 `description` 仅是 JSON Schema 中描述参数的元数据，不会被 LLM 作为参数传入。

---

## 2. 相关文件索引

| 文件 | 作用 |
|------|------|
| `cli/src/run/agent.rs` | CLI spinner 显示工具名 + description |
| `cli/src/args.rs` | `--verbose` 参数定义 |
| `loom/src/stream_display/event_handler.rs` | 通用流式事件处理（CLI 和 lib 共用） |
| `loom-acp/src/stream_bridge.rs` | Loom 事件 → ACP SessionUpdate 映射 |
| `telegram-bot/src/streaming/event_mapper.rs` | Telegram 事件回调（已 no-op） |
| `web/packages/hooks/src/useChat.ts` | Web 前端 WebSocket 消息处理 |
| `web/packages/adapters/src/ToolBlockAdapter.ts` | 工具块 UI 状态映射 |
| `web/packages/adapters/src/ToolStreamAggregator.ts` | 工具流事件聚合器 |
