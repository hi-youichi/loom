---
sidebar_position: 2
title: "CLI JSON 流式输出"
description: "使用 --json 标志获取机器可读的 JSONL 事件流，用于自动化脚本和 CI/CD 集成"
---

# CLI JSON 流式输出

> **⚠️ 注意**：本文档描述的 Codex CLI 的 `--json` 事件协议 (`thread.started` / `turn.started` / `item.started` 等)。Loom 的 `--json` 输出使用不同的 `ProtocolEvent` 格式（`node_enter` / `message_chunk` / `tool_call` 等），文档正在更新中。

Codex CLI 的 `--json` 标志启用机器可读的 JSONL（每行一个 JSON 对象）流式输出，适用于自动化脚本和 CI/CD 集成。

**关键特性：**

- stdout 严格仅输出 JSONL，日志和警告输出到 stderr
- 事件类型使用 `type` 字段区分
- item ID 格式为 `item_0`、`item_1`、`item_2` ...
- 支持轮次追踪、token 使用统计、工具调用等详细信息

## 启用方式

```bash
codex exec --json "你的提示词"
codex exec --json --output-last-message result.json "生成代码"
```

`--output-last-message` 参数会将最后一个 `agent_message` 事件的文本内容写入指定文件。如果轮次中没有产生 `agent_message`，文件不会被创建。

## 事件流结构

### 核心事件类型

| 事件名 `type` | 触发时机 | 主要字段 |
|--------------|----------|----------|
| `thread.started` | 线程创建时 | `thread_id` |
| `turn.started` | 每轮对话开始 | 无额外字段 |
| `turn.completed` | 轮次完成时 | `usage: {input_tokens, output_tokens, ...}` |
| `turn.failed` | 轮次失败时 | `error: {message}` |
| `item.started` | 新操作开始 | `item: {id, type, ...}` |
| `item.updated` | 操作状态更新 | `item: {id, type, ...}` |
| `item.completed` | 操作完成或失败 | `item: {id, type, ...}` |
| `error` | 不可恢复错误 | `message` |

### item.updated 说明

`item.updated` 在以下场景触发：

- **长命令执行中**：`command_execution` 类型会在执行过程中持续发送 `item.updated`，携带最新的 `aggregated_output`
- **待办事项变更**：`todo_list` 类型在步骤状态变更时发送
- **协作工具调用**：`collab_tool_call` 在代理状态变化时发送

### ThreadItem 类型

每种 ThreadItem 都有 `type` 字段区分：

| 类型 `type` | 说明 | 关键字段 |
|-----------|------|----------|
| `agent_message` | 模型回复文本 | `text` |
| `reasoning` | 推理摘要 | `text` |
| `command_execution` | 命令执行 | `command`、`aggregated_output`、`exit_code`、`status` |
| `file_change` | 文件变更 | `changes: [{path, kind}]`、`status` |
| `mcp_tool_call` | MCP 工具调用 | `server`、`tool`、`arguments`、`result`、`error`、`status` |
| `collab_tool_call` | 协作工具调用 | `tool`、`sender_thread_id`、`receiver_thread_ids`、`status` |
| `web_search` | 网络搜索 | `id`、`query`、`action` |
| `todo_list` | 待办事项列表 | `items: [{text, completed}]` |
| `error` | 非致命错误 | `message` |

## 主要数据结构

### ThreadStartedEvent

```rust
struct ThreadStartedEvent {
    thread_id: String,
}
```

### TurnCompletedEvent

```rust
struct TurnCompletedEvent {
    usage: Usage {
        input_tokens: i64,
        cached_input_tokens: i64,
        output_tokens: i64,
        reasoning_output_tokens: i64,
    },
}
```

### TurnFailedEvent

```rust
struct TurnFailedEvent {
    error: ThreadErrorEvent {
        message: String,
    },
}
```

### CommandExecutionItem

```rust
struct CommandExecutionItem {
    command: String,
    aggregated_output: String,
    exit_code: Option<i32>,
    status: CommandExecutionStatus, // InProgress | Completed | Failed | Declined
}
```

### FileChangeItem

```rust
struct FileChangeItem {
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus, // InProgress | Completed | Failed
}

struct FileUpdateChange {
    path: String,
    kind: PatchChangeKind, // Add | Delete | Update
}
```

### McpToolCallItem

```rust
struct McpToolCallItem {
    server: String,
    tool: String,
    arguments: JsonValue,
    result: Option<McpToolCallItemResult>,
    error: Option<McpToolCallItemError>,
    status: McpToolCallStatus, // InProgress | Completed | Failed
}
```

## 事件流示例

### 基础推理与命令执行

```json
{"type":"thread.started","thread_id":"thread_12345"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_0","type":"reasoning","text":"分析需求..."}}
{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"分析完成"}}
{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"ls -la","aggregated_output":"","exit_code":null,"status":"in_progress"}}
{"type":"item.updated","item":{"id":"item_1","type":"command_execution","command":"ls -la","aggregated_output":"total 24\ndrwxr-xr-x","exit_code":null,"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"ls -la","aggregated_output":"total 24\ndrwxr-xr-x  5 user staff  160 Aug 19 10:00 .","exit_code":0,"status":"completed"}}
{"type":"turn.completed","usage":{"input_tokens":150,"cached_input_tokens":0,"output_tokens":75,"reasoning_output_tokens":50}}
```

### 文件变更与消息回复

```json
{"type":"thread.started","thread_id":"thread_abcde"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_0","type":"file_change","changes":[{"path":"src/main.rs","kind":"Update"},{"path":"src/utils.rs","kind":"Add"}],"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_0","type":"file_change","changes":[{"path":"src/main.rs","kind":"Update"},{"path":"src/utils.rs","kind":"Add"}],"status":"completed"}}
{"type":"item.started","item":{"id":"item_1","type":"agent_message","text":"已创建 utils.rs 并更新 main.rs"}}
{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"已创建 utils.rs 并更新 main.rs"}}
{"type":"turn.completed","usage":{"input_tokens":200,"cached_input_tokens":0,"output_tokens":100,"reasoning_output_tokens":30}}
```

## 使用场景

### 1. 脚本集成

```bash
#!/bin/bash
OUTPUT=$(codex exec --json "生成 Python API" | jq -r 'select(.item.type == "agent_message" and .type == "item.completed") | .item.text')
echo "$OUTPUT"
```

### 2. CI/CD 集成

```yaml
- name: Code Review
  run: |
    codex exec --json "review this code" | jq 'select(.type == "turn.failed") | .error.message' > review_errors.json
    if [ -s review_errors.json ]; then exit 1; fi
```

### 3. 提取特定事件

```bash
# 提取所有文件变更
codex exec --json "修改配置" | jq 'select(.item.type == "file_change")'

# 提取 token 使用情况
codex exec --json "分析" | jq 'select(.type == "turn.completed") | .usage'

# 检查是否失败
codex exec --json "任务" | jq -e 'select(.type == "turn.failed")'
```

### 4. 错误处理

消费端处理错误的推荐方式：

- 收到 `turn.failed` 事件：轮次级别失败，可检查 `error.message` 决定是否重试
- 收到 `error` 事件：不可恢复错误，应终止处理
- `item.completed` 中 `status` 为 `failed`：单项失败，不影响整体流程

```bash
#!/bin/bash
codex exec --json "任务" 2>/dev/null | while read -r line; do
  TYPE=$(echo "$line" | jq -r '.type')
  case "$TYPE" in
    "error")
      echo "不可恢复错误: $(echo "$line" | jq -r '.message')"
      exit 1
      ;;
    "turn.failed")
      echo "轮次失败: $(echo "$line" | jq -r '.error.message')"
      exit 1
      ;;
    "item.completed")
      STATUS=$(echo "$line" | jq -r '.item.status // "completed"')
      if [ "$STATUS" = "failed" ]; then
        echo "操作失败: $(echo "$line" | jq -c '.item')"
      fi
      ;;
  esac
done
```

## 技术细节

### Item ID 映射机制

- 内部 raw ID 映射为 `item_0`、`item_1` 等稳定 ID
- 使用 `HashMap<String, String>` 进行映射
- 保证跨事件的 item 引用一致性

### 状态转换

- `item.started` → `item.updated` → `item.completed`
- `turn.started` → 多个 item → `turn.completed` / `turn.failed`
- 错误时发送 `error` 事件

### 输出隔离

- 使用 `#![deny(clippy::print_stdout)]` 强制 stdout 只输出 JSONL
- 所有日志使用 `eprintln!` 输出到 stderr
- 便于流水线处理：`codex exec --json ... 2>/dev/null`

## 代码实现参考

| 组件 | 文件路径 |
|------|----------|
| CLI 标志定义 | `exec/src/cli.rs` |
| 事件类型与数据结构 | `exec/src/exec_events.rs` |
| JSONL 输出处理器 | `exec/src/event_processor_with_jsonl_output.rs` |
| 事件处理器分发 | `exec/src/lib.rs` |

---

**相关文档**: [CLI 安装与配置](./cli.md) | [Troubleshooting](./troubleshooting.md)
