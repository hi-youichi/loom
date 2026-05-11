---
sidebar_position: 4
title: "Codex 事件协议字段参考"
description: "Codex CLI JSONL 事件流的完整 JSON Schema 和字段级参考，涵盖所有事件类型、ThreadItem 类型和嵌套结构"
---

# Codex 事件协议字段参考

本文档是 `codex exec --json` 输出的完整字段级参考。每种类型包含 JSON Schema 示例和逐字段说明。

> **源码**：`thirdparty/codex/codex-rs/exec/src/exec_events.rs`
> **使用指南**：[CLI JSON 流式输出](../deployment/cli-json-output.md)

## 通用约定

| 约定 | 说明 |
|------|------|
| 格式 | JSONL：每行一个 JSON 对象 |
| `type` 字段 | 所有顶层事件和 ThreadItem 都通过 `type` 字段区分变体 |
| 命名风格 | 字段名 `snake_case`；枚举值 `snake_case` |
| Item ID | 稳定格式 `item_0`、`item_1`、`item_2` ... |
| stdout 隔离 | stdout 仅输出 JSONL；日志和警告输出到 stderr |

## 事件生命周期

```
thread.started                          ← 会话开始（仅一次）
  └── turn.started                      ← 每轮对话开始
        ├── item.started                ← 操作开始
        │     ├── item.updated          ← 操作更新（0..N 次）
        │     └── item.completed        ← 操作结束
        ├── item.started ...
        └── turn.completed | turn.failed
  └── turn.started ...                  ← 多轮对话
error                                   ← 不可恢复错误（任意时刻）
```

---

## 1. 顶层事件

### 1.1 thread.started

线程创建时发送的第一个事件。

```json
{
  "type": "thread.started",
  "thread_id": "thread_abc123def456"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"thread.started"` | ✅ | 事件类型标识 |
| `thread_id` | `string` | ✅ | 线程唯一标识符，可用于恢复会话 |

### 1.2 turn.started

每轮对话开始时发送。一个轮次包含从发送 prompt 到模型完成响应的全部事件。

```json
{
  "type": "turn.started"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"turn.started"` | ✅ | 事件类型标识 |

无额外字段。

### 1.3 turn.completed

轮次正常完成时发送，携带 token 使用统计。

```json
{
  "type": "turn.completed",
  "usage": {
    "input_tokens": 1520,
    "cached_input_tokens": 800,
    "output_tokens": 475,
    "reasoning_output_tokens": 120
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"turn.completed"` | ✅ | 事件类型标识 |
| `usage` | `object` | ✅ | Token 使用统计，见 [Usage](#5-1-usage) |

### 1.4 turn.failed

轮次因错误而终止时发送。

```json
{
  "type": "turn.failed",
  "error": {
    "message": "Model request failed: rate limit exceeded"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"turn.failed"` | ✅ | 事件类型标识 |
| `error` | `object` | ✅ | 错误详情，见 [Error 事件](#18-error) |

### 1.5 item.started

新操作开始时发送，携带 ThreadItem 的初始状态（通常为 `in_progress`）。

```json
{
  "type": "item.started",
  "item": {
    "id": "item_0",
    "type": "command_execution",
    "command": "cargo test",
    "aggregated_output": "",
    "exit_code": null,
    "status": "in_progress"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"item.started"` | ✅ | 事件类型标识 |
| `item` | `object` | ✅ | ThreadItem，见 [ThreadItem](#2-threaditem-通用结构) |

### 1.6 item.updated

操作状态更新时发送。触发场景：

- `command_execution`：长命令执行中持续发送，更新 `aggregated_output`
- `todo_list`：步骤状态变更
- `collab_tool_call`：代理状态变化

```json
{
  "type": "item.updated",
  "item": {
    "id": "item_0",
    "type": "command_execution",
    "command": "cargo test",
    "aggregated_output": "running 3 tests\n...",
    "exit_code": null,
    "status": "in_progress"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"item.updated"` | ✅ | 事件类型标识 |
| `item` | `object` | ✅ | ThreadItem 的最新状态 |

### 1.7 item.completed

操作到达终态（成功或失败）时发送。

```json
{
  "type": "item.completed",
  "item": {
    "id": "item_0",
    "type": "command_execution",
    "command": "cargo test",
    "aggregated_output": "running 3 tests\ntest result: ok. 3 passed; 0 failed",
    "exit_code": 0,
    "status": "completed"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"item.completed"` | ✅ | 事件类型标识 |
| `item` | `object` | ✅ | ThreadItem 的最终状态 |

### 1.8 error

不可恢复错误，可在事件流的任意时刻发送。收到此事件后不应继续处理后续事件。

```json
{
  "type": "error",
  "message": "Authentication failed: invalid API key"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"error"` | ✅ | 事件类型标识 |
| `message` | `string` | ✅ | 错误描述 |

---

## 2. ThreadItem 通用结构

所有 `item` 字段共享以下结构：

```json
{
  "id": "item_0",
  "type": "<thread_item_type>",
  "...type_specific_fields..."
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | 稳定 ID：`item_0`、`item_1` ... 跨事件保持一致 |
| `type` | `string` | ✅ | ThreadItem 类型标识，决定其他字段 |
| *其余字段* | — | — | 由 `type` 决定，见下方各类型 |

**`type` 字段通过 `serde(flatten)` 扁平化到同一层级**，因此 `type` 与类型特定字段位于同一个 JSON 对象中。

---

## 3. ThreadItem 类型

### 3.1 agent_message

模型的文本回复。可能是自然语言或结构化 JSON（当使用 `--output-schema` 时）。

```json
{
  "id": "item_1",
  "type": "agent_message",
  "text": "已成功创建 src/utils.rs 并更新 main.rs 中的导入。"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"agent_message"` | ✅ | 类型标识 |
| `text` | `string` | ✅ | 回复文本内容 |

状态流转：`item.started` → `item.completed`（无中间 `item.updated`）

### 3.2 reasoning

模型的推理摘要，展示思考过程。

```json
{
  "id": "item_0",
  "type": "reasoning",
  "text": "用户要求创建一个工具函数。需要检查现有文件结构..."
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"reasoning"` | ✅ | 类型标识 |
| `text` | `string` | ✅ | 推理摘要文本 |

状态流转：`item.started` → `item.completed`

### 3.3 command_execution

Agent 执行的 shell 命令。从命令启动到进程退出全程追踪。

```json
{
  "id": "item_2",
  "type": "command_execution",
  "command": "cargo clippy -- -D warnings",
  "aggregated_output": "Checking myproject v0.1.0\nwarning: unused variable\n...",
  "exit_code": 1,
  "status": "failed"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"command_execution"` | ✅ | 类型标识 |
| `command` | `string` | ✅ | 执行的 shell 命令 |
| `aggregated_output` | `string` | ✅ | 累积的 stdout + stderr 输出 |
| `exit_code` | `integer \| null` | ✅ | 进程退出码。执行中为 `null`，结束后为整数 |
| `status` | `string` | ✅ | 执行状态，见 [CommandExecutionStatus](#5-2-commandexecutionstatus) |

状态流转：

```
in_progress → completed    (exit_code = 0)
            → failed       (exit_code ≠ 0)
            → declined     (用户拒绝执行)
```

`item.updated` 在执行过程中持续触发，`aggregated_output` 逐步增长。

### 3.4 file_change

Agent 对文件系统的变更。仅在 patch 成功或失败时发送 `item.completed`（无 `item.started`）。

```json
{
  "id": "item_3",
  "type": "file_change",
  "changes": [
    { "path": "src/utils.rs", "kind": "add" },
    { "path": "src/main.rs", "kind": "update" },
    { "path": "src/old_module.rs", "kind": "delete" }
  ],
  "status": "completed"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"file_change"` | ✅ | 类型标识 |
| `changes` | `array` | ✅ | 文件变更列表，见 [FileUpdateChange](#4-1-fileupdatechange) |
| `status` | `string` | ✅ | 应用状态，见 [PatchApplyStatus](#5-3-patchapplystatus) |

状态流转：

```
in_progress → completed    (所有 patch 成功应用)
            → failed       (patch 应用失败)
```

### 3.5 mcp_tool_call

Agent 对 MCP（Model Context Protocol）工具的调用。

```json
{
  "id": "item_4",
  "type": "mcp_tool_call",
  "server": "filesystem",
  "tool": "read_file",
  "arguments": { "path": "/src/main.rs" },
  "result": {
    "content": [{ "type": "text", "text": "fn main() { ... }" }],
    "structured_content": null
  },
  "error": null,
  "status": "completed"
}
```

失败示例：

```json
{
  "id": "item_5",
  "type": "mcp_tool_call",
  "server": "filesystem",
  "tool": "write_file",
  "arguments": { "path": "/etc/hosts", "content": "..." },
  "result": null,
  "error": { "message": "Permission denied: /etc/hosts is outside writable roots" },
  "status": "failed"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"mcp_tool_call"` | ✅ | 类型标识 |
| `server` | `string` | ✅ | MCP 服务器名称 |
| `tool` | `string` | ✅ | 工具名称 |
| `arguments` | `any` (JSON) | ✅ | 工具调用参数（任意 JSON 值，默认 `{}`） |
| `result` | `object \| null` | ✅ | 调用结果，见 [McpToolCallItemResult](#4-2-mcptoolcallitemresult) |
| `error` | `object \| null` | ✅ | 错误详情，见 [McpToolCallItemError](#4-3-mcptoolcallitemerror) |
| `status` | `string` | ✅ | 调用状态，见 [McpToolCallStatus](#5-5-mcptoolcallstatus) |

`result` 和 `error` 互斥：成功时 `result` 非空且 `error` 为 `null`，失败时相反。

状态流转：

```
in_progress → completed    (result 非空)
            → failed       (error 非空)
```

### 3.6 collab_tool_call

Agent 调用协作工具（多代理系统）。

```json
{
  "id": "item_6",
  "type": "collab_tool_call",
  "tool": "spawn_agent",
  "sender_thread_id": "thread_abc123",
  "receiver_thread_ids": ["thread_def456"],
  "prompt": "分析 src/parser.rs 的错误处理逻辑",
  "agents_states": {
    "thread_def456": {
      "status": "running",
      "message": null
    }
  },
  "status": "in_progress"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"collab_tool_call"` | ✅ | 类型标识 |
| `tool` | `string` | ✅ | 协作工具类型，见 [CollabTool](#5-6-collabtool) |
| `sender_thread_id` | `string` | ✅ | 发起方线程 ID |
| `receiver_thread_ids` | `string[]` | ✅ | 接收方线程 ID 列表 |
| `prompt` | `string \| null` | ✅ | 发送给接收方的提示词（`spawn_agent` 和 `send_input` 时使用） |
| `agents_states` | `object` | ✅ | 各代理的状态映射，键为线程 ID，值为 [CollabAgentState](#4-4-collabagentstate) |
| `status` | `string` | ✅ | 调用状态，见 [CollabToolCallStatus](#5-7-collabtoolcallstatus) |

状态流转：

```
in_progress → completed    (所有代理完成)
            → failed       (代理错误)
```

`item.updated` 在代理状态变化时触发，`agents_states` 持续更新。

### 3.7 web_search

Agent 发起的网络搜索请求。

```json
{
  "id": "item_7",
  "type": "web_search",
  "query": "Rust async best practices 2025",
  "action": {
    "type": "search",
    "query": "Rust async best practices 2025",
    "queries": null
  }
}
```

打开页面示例：

```json
{
  "id": "item_8",
  "type": "web_search",
  "query": "Open https://doc.rust-lang.org/book/",
  "action": {
    "type": "open_page",
    "url": "https://doc.rust-lang.org/book/"
  }
}
```

页面内搜索示例：

```json
{
  "id": "item_9",
  "type": "web_search",
  "query": "Find trait bounds in page",
  "action": {
    "type": "find_in_page",
    "url": "https://doc.rust-lang.org/book/ch10-02-traits.html",
    "pattern": "trait bound"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"web_search"` | ✅ | 类型标识 |
| `query` | `string` | ✅ | 搜索查询文本 |
| `action` | `object` | ✅ | 搜索动作详情，见 [WebSearchAction](#4-5-websearchaction) |

### 3.8 todo_list

Agent 的待办事项列表，追踪任务分解和进度。

```json
{
  "id": "item_10",
  "type": "todo_list",
  "items": [
    { "text": "分析现有代码结构", "completed": true },
    { "text": "创建 utils 模块", "completed": true },
    { "text": "编写单元测试", "completed": false },
    { "text": "更新文档", "completed": false }
  ]
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"todo_list"` | ✅ | 类型标识 |
| `items` | `array` | ✅ | 待办事项列表，见 [TodoItem](#4-6-todoitem) |

状态流转：`item.started`（初始计划）→ `item.updated`（步骤状态变更，0..N 次）→ `item.completed`（轮次结束）

### 3.9 error

非致命错误，作为 ThreadItem 出现在事件流中。不影响轮次继续。

```json
{
  "id": "item_11",
  "type": "error",
  "message": "MCP server 'database' is not responding, skipping related tool calls"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | `string` | ✅ | Item 稳定 ID |
| `type` | `"error"` | ✅ | 类型标识 |
| `message` | `string` | ✅ | 错误描述 |

与顶层 `error` 事件的区别：

| | ThreadItem `error` | 顶层 `error` 事件 |
|---|---|---|
| 严重程度 | 非致命，流程继续 | 不可恢复，流程终止 |
| 出现位置 | `item` 字段内 | 顶层事件 |
| 有 `id` | ✅ | ❌ |

---

## 4. 嵌套类型

### 4-1 FileUpdateChange

文件变更的单条记录。

```json
{ "path": "src/main.rs", "kind": "update" }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `path` | `string` | ✅ | 文件路径（相对于工作目录） |
| `kind` | `string` | ✅ | 变更类型：`"add"` / `"delete"` / `"update"` |

### 4-2 McpToolCallItemResult

MCP 工具调用的返回结果。

```json
{
  "content": [
    { "type": "text", "text": "File contents here..." }
  ],
  "structured_content": null
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `content` | `any[]` (JSON) | ✅ | MCP 内容块数组。具体结构由 MCP 协议定义 |
| `structured_content` | `any \| null` (JSON) | ✅ | 结构化内容（若 MCP 工具返回结构化数据） |

### 4-3 McpToolCallItemError

MCP 工具调用的错误信息。

```json
{ "message": "Permission denied: /etc/hosts is outside writable roots" }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `message` | `string` | ✅ | 错误描述 |

### 4-4 CollabAgentState

协作代理的当前状态。

```json
{ "status": "running", "message": null }
```

```json
{ "status": "errored", "message": "Process exited with code 1" }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `status` | `string` | ✅ | 代理状态，见 [CollabAgentStatus](#5-8-collabagentstatus) |
| `message` | `string \| null` | ✅ | 附加消息（如错误详情） |

### 4-5 WebSearchAction

网络搜索的具体动作。通过 `type` 字段区分三种变体。

#### search — 关键词搜索

```json
{
  "type": "search",
  "query": "Rust async best practices",
  "queries": null
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"search"` | ✅ | 动作类型 |
| `query` | `string \| null` | ❌ | 搜索查询词 |
| `queries` | `string[] \| null` | ❌ | 多个查询词 |

#### open_page — 打开网页

```json
{
  "type": "open_page",
  "url": "https://doc.rust-lang.org/book/"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"open_page"` | ✅ | 动作类型 |
| `url` | `string \| null` | ❌ | 目标 URL |

#### find_in_page — 页面内搜索

```json
{
  "type": "find_in_page",
  "url": "https://doc.rust-lang.org/book/ch10-02-traits.html",
  "pattern": "trait bound"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | `"find_in_page"` | ✅ | 动作类型 |
| `url` | `string \| null` | ❌ | 搜索的目标页面 URL |
| `pattern` | `string \| null` | ❌ | 搜索模式 |

#### other — 未知动作

当遇到无法识别的动作类型时的兜底值。无额外字段。

```json
{ "type": "unknown_action_type" }
```

### 4-6 TodoItem

待办事项的单条记录。

```json
{ "text": "编写单元测试", "completed": false }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `text` | `string` | ✅ | 待办事项描述 |
| `completed` | `boolean` | ✅ | 是否已完成 |

---

## 5. Stream Delta 通知（v2 协议层）

> **重要**：以下 Delta 通知属于 v2 协议层（`app-server-protocol`），用于 TUI 和外部客户端的实时流式更新。`codex exec --json` 输出**不直接转发**这些 Delta，而是将其聚合为 `item.updated` 事件中的完整状态。如果你的集成需要逐 token 实时流式输出，应直接连接 v2 协议端点。

### 5-1 AgentMessageDelta — 模型文本增量

模型生成文本时逐 token 发送。

```json
{
  "method": "item/agentMessage/delta",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_0",
    "delta": "已"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | 消息 Item ID |
| `delta` | `string` | ✅ | 文本增量（单个或多个 token） |

消费端应拼接所有 `delta` 得到完整文本。最终完整文本由 `item.completed` 中的 `agent_message.text` 确认。

### 5-2 ReasoningSummaryTextDelta — 推理摘要增量

推理摘要生成时逐 token 发送。

```json
{
  "method": "item/reasoning/summaryTextDelta",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_0",
    "delta": "分析用户",
    "summaryIndex": 0
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | 推理 Item ID |
| `delta` | `string` | ✅ | 摘要文本增量 |
| `summaryIndex` | `integer` | ✅ | 摘要段落索引（对应 `reasoning.summary[]` 的下标） |

### 5-3 ReasoningSummaryPartAdded — 推理摘要新增段落

当推理摘要新增一个段落时发送。

```json
{
  "method": "item/reasoning/summaryPartAdded",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_0",
    "summaryIndex": 1
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | 推理 Item ID |
| `summaryIndex` | `integer` | ✅ | 新增段落的索引 |

消费端应在收到此通知后准备接收该 `summaryIndex` 对应的 `ReasoningSummaryTextDelta`。

### 5-4 ReasoningTextDelta — 推理原始内容增量

推理的原始内容（未经摘要处理）逐 token 发送。

```json
{
  "method": "item/reasoning/textDelta",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_0",
    "delta": "thinking...",
    "contentIndex": 0
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | 推理 Item ID |
| `delta` | `string` | ✅ | 原始内容文本增量 |
| `contentIndex` | `integer` | ✅ | 内容块索引（对应 `reasoning.content[]` 的下标） |

### 5-5 CommandExecutionOutputDelta — 命令输出增量

命令执行过程中 stdout/stderr 的增量输出。

```json
{
  "method": "item/commandExecution/outputDelta",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_1",
    "delta": "running 3 tests\n"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | 命令执行 Item ID |
| `delta` | `string` | ✅ | 输出增量文本 |

与 `exec --json` 的关系：`exec` 将所有 delta 聚合为 `command_execution.aggregated_output`，通过 `item.updated` 发送。

### 5-6 FileChangePatchUpdated — 文件变更实时更新

文件变更过程中 patch 内容的实时更新。

```json
{
  "method": "item/fileChange/patchUpdated",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_2",
    "changes": [
      { "path": "src/main.rs", "kind": { "type": "update" }, "diff": "--- a/src/main.rs\n..." }
    ]
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | 文件变更 Item ID |
| `changes` | `array` | ✅ | 文件变更列表，见 [FileUpdateChange](#4-1-fileupdatechange) |

与 `exec --json` 的关系：`exec` 只在 `item.completed` 中发送最终变更列表。

### 5-7 PlanDelta — 计划文本增量（实验性）

Agent 计划生成时逐 token 发送。

```json
{
  "method": "item/plan/delta",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_0",
    "delta": "1. 检查编译错误\n"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | 计划 Item ID |
| `delta` | `string` | ✅ | 计划文本增量 |

> **注意**：这是实验性功能。拼接的 delta 不保证与最终 `Plan` item 的 `text` 完全一致，应以 `item.completed` 为准。

### 5-8 TerminalInteraction — 终端交互输入

用户在运行中的终端进程里输入内容时发送。

```json
{
  "method": "item/commandExecution/terminalInteraction",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_1",
    "processId": "proc_456",
    "stdin": "y\n"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | 命令执行 Item ID |
| `processId` | `string` | ✅ | 进程标识符 |
| `stdin` | `string` | ✅ | 用户输入内容 |

### 5-9 McpToolCallProgress — MCP 工具调用进度

MCP 工具执行过程中的中间进度报告。

```json
{
  "method": "item/mcpToolCall/progress",
  "params": {
    "threadId": "thread_abc123",
    "turnId": "turn_001",
    "itemId": "item_3",
    "message": "Processing 42 of 100 files..."
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `threadId` | `string` | ✅ | 线程 ID |
| `turnId` | `string` | ✅ | 轮次 ID |
| `itemId` | `string` | ✅ | MCP 工具调用 Item ID |
| `message` | `string` | ✅ | 进度描述文本 |

### 5-10 CommandExecOutputDelta — 独立命令执行输出增量

通过 `command/exec` 直接执行命令时的 base64 编码输出增量。

```json
{
  "method": "command/exec/outputDelta",
  "params": {
    "processId": "proc_789",
    "stream": "stdout",
    "deltaBase64": "SGVsbG8gV29ybGQK",
    "capReached": false
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `processId` | `string` | ✅ | 客户端提供的进程标识符 |
| `stream` | `string` | ✅ | 输出流：`"stdout"` 或 `"stderr"` |
| `deltaBase64` | `string` | ✅ | Base64 编码的输出字节 |
| `capReached` | `boolean` | ✅ | 是否因 `outputBytesCap` 截断（最后一 chunk 为 `true`） |

### 5-11 ProcessOutputDelta — 进程输出增量（实验性）

通过 `process/spawn` 管理的进程的 base64 编码输出增量。

```json
{
  "method": "process/outputDelta",
  "params": {
    "processHandle": "handle_xyz",
    "stream": "stdout",
    "deltaBase64": "YnVpbGRpbmcuLi4K",
    "capReached": false
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `processHandle` | `string` | ✅ | 客户端提供的进程句柄 |
| `stream` | `string` | ✅ | 输出流：`"stdout"` 或 `"stderr"` |
| `deltaBase64` | `string` | ✅ | Base64 编码的输出字节 |
| `capReached` | `boolean` | ✅ | 是否因 `outputBytesCap` 截断 |

### Delta 与 exec --json 的映射关系

| v2 协议 Delta | exec --json 等价事件 |
|--------------|---------------------|
| `AgentMessageDelta` × N | `item.completed` (agent_message, 完整文本) |
| `ReasoningSummaryTextDelta` × N | `item.completed` (reasoning, 合并后文本) |
| `ReasoningSummaryPartAdded` | — (exec 聚合后输出) |
| `ReasoningTextDelta` × N | — (exec 不输出原始推理内容) |
| `CommandExecutionOutputDelta` × N | `item.updated` (aggregated_output 逐步增长) |
| `FileChangePatchUpdated` | `item.completed` (file_change, 最终变更列表) |
| `PlanDelta` × N | — (exec 不输出 Plan item) |
| `TerminalInteraction` | — (exec 非交互模式不产生) |
| `McpToolCallProgress` | — (exec 不输出进度) |
| `CommandExecOutputDelta` | — (独立命令执行，非 Agent 调用) |
| `ProcessOutputDelta` | — (进程管理，非 Agent 调用) |

---

## 7. 枚举类型汇总

### 5-1 Usage

Token 使用统计，出现在 `turn.completed` 事件中。

| 字段 | 类型 | 说明 |
|------|------|------|
| `input_tokens` | `integer` | 输入 token 数 |
| `cached_input_tokens` | `integer` | 缓存命中的输入 token 数 |
| `output_tokens` | `integer` | 输出 token 数 |
| `reasoning_output_tokens` | `integer` | 推理输出 token 数 |

### 5-2 CommandExecutionStatus

`command_execution` 类型 `status` 字段的枚举值。

| 值 | 说明 |
|---|------|
| `in_progress` | 命令执行中 |
| `completed` | 命令正常退出 |
| `failed` | 命令以非零退出码结束 |
| `declined` | 用户拒绝执行该命令 |

### 5-3 PatchApplyStatus

`file_change` 类型 `status` 字段的枚举值。

| 值 | 说明 |
|---|------|
| `in_progress` | Patch 应用中 |
| `completed` | Patch 成功应用 |
| `failed` | Patch 应用失败 |

### 5-4 PatchChangeKind

`FileUpdateChange` 中 `kind` 字段的枚举值。

| 值 | 说明 |
|---|------|
| `add` | 新建文件 |
| `delete` | 删除文件 |
| `update` | 修改文件内容 |

### 5-5 McpToolCallStatus

`mcp_tool_call` 类型 `status` 字段的枚举值。

| 值 | 说明 |
|---|------|
| `in_progress` | MCP 工具调用中 |
| `completed` | 调用成功，结果在 `result` 字段 |
| `failed` | 调用失败，详情在 `error` 字段 |

### 5-6 CollabTool

`collab_tool_call` 类型 `tool` 字段的枚举值。

| 值 | 说明 |
|---|------|
| `spawn_agent` | 启动子代理 |
| `send_input` | 向子代理发送输入 |
| `wait` | 等待子代理完成 |
| `close_agent` | 关闭子代理 |

### 5-7 CollabToolCallStatus

`collab_tool_call` 类型 `status` 字段的枚举值。

| 值 | 说明 |
|---|------|
| `in_progress` | 协作调用进行中 |
| `completed` | 所有代理已完成 |
| `failed` | 代理执行失败 |

### 5-8 CollabAgentStatus

`CollabAgentState` 中 `status` 字段的枚举值。

| 值 | 说明 |
|---|------|
| `pending_init` | 等待初始化 |
| `running` | 运行中 |
| `interrupted` | 被中断 |
| `completed` | 已完成 |
| `errored` | 出错 |
| `shutdown` | 已关闭 |
| `not_found` | 未找到 |

---

## 8. 完整事件流示例

以下是一个完整的 `codex exec --json` 会话，Agent 分析代码、执行命令、修改文件并回复用户。

```json
{"type":"thread.started","thread_id":"thread_a1b2c3d4"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_0","type":"reasoning","text":"用户要求修复编译错误。先检查错误信息..."}}
{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"用户要求修复编译错误。先检查错误信息..."}}
{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"cargo build 2>&1","aggregated_output":"","exit_code":null,"status":"in_progress"}}
{"type":"item.updated","item":{"id":"item_1","type":"command_execution","command":"cargo build 2>&1","aggregated_output":"   Compiling myproject v0.1.0\nerror[E0425]: cannot find value `config` in this scope\n  --> src/main.rs:42:5\n","exit_code":null,"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"cargo build 2>&1","aggregated_output":"   Compiling myproject v0.1.0\nerror[E0425]: cannot find value `config` in this scope\n  --> src/main.rs:42:5\n   |\n42 |     config.load()\n   |     ^^^^^^ not found in this scope\n\nerror: aborting due to 1 previous error\n","exit_code":101,"status":"failed"}}
{"type":"item.started","item":{"id":"item_2","type":"file_change","changes":[{"path":"src/main.rs","kind":"update"}],"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_2","type":"file_change","changes":[{"path":"src/main.rs","kind":"update"}],"status":"completed"}}
{"type":"item.started","item":{"id":"item_3","type":"command_execution","command":"cargo build 2>&1","aggregated_output":"","exit_code":null,"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_3","type":"command_execution","command":"cargo build 2>&1","aggregated_output":"   Compiling myproject v0.1.0\n    Finished dev [unoptimized + debuginfo] target(s) in 1.2s\n","exit_code":0,"status":"completed"}}
{"type":"item.started","item":{"id":"item_4","type":"agent_message","text":"已修复编译错误。问题是 `config` 变量未在作用域内声明，已在 src/main.rs:40 添加了变量声明。构建验证通过。"}}
{"type":"item.completed","item":{"id":"item_4","type":"agent_message","text":"已修复编译错误。问题是 `config` 变量未在作用域内声明，已在 src/main.rs:40 添加了变量声明。构建验证通过。"}}
{"type":"turn.completed","usage":{"input_tokens":1520,"cached_input_tokens":800,"output_tokens":475,"reasoning_output_tokens":120}}
```

**事件流解读**：

| 行 | 事件 | 说明 |
|----|------|------|
| 1 | `thread.started` | 会话开始 |
| 2 | `turn.started` | 第一轮对话开始 |
| 3-4 | `item_0` reasoning | Agent 推理：决定先检查编译错误 |
| 5-7 | `item_1` command | 执行 `cargo build`，得到错误输出 |
| 8-9 | `item_2` file_change | 修改 `src/main.rs` |
| 10-11 | `item_3` command | 再次构建验证修复 |
| 12-13 | `item_4` agent_message | Agent 向用户报告结果 |
| 14 | `turn.completed` | 轮次结束，附带 token 统计 |

---

## 相关文档

- [CLI JSON 流式输出](../deployment/cli-json-output.md) — JSONL 的启用方式和使用场景
- [Codex CLI 参考手册](./codex-reference.md) — 命令行选项和配置体系概览
- [Claude Code JSON 协议参考](./claude-code-json-protocol.md) — Claude Code CLI 的 JSON 输出协议
