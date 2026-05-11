# Codex CLI JSON 流式输出文档

## 概述

Codex CLI 的 `--json` 标志启用机器可读的 JSONL（每行一个 JSON 对象）流式输出，适用于自动化脚本和 CI/CD 集成。

**关键特性：**
- stdout 严格仅输出 JSONL，日志/警告输出到 stderr
- 事件类型使用 `type` 字段区分
- item ID 格式为 `item_0`, `item_1`, `item_2` ...
- 支持轮次追踪、token 使用统计、工具调用等详细信息

## 启用方式

```bash
codex exec --json "你的提示词"
codex exec --json --output-last-message result.json "生成代码"
```

## 事件流结构

### 核心事件类型

| 事件名 `type` | 触发时机 | 主要字段 |
|--------------|----------|----------|
| `thread.started` | 线程创建时 | `thread_id` |
| `turn.started` | 每轮对话开始 | 无额外字段 |
| `turn.completed` | 轮次完成时 | `usage: {input_tokens, output_tokens, ...}` |
| `turn.failed` | 轮次失败时 | `error: {message}` |
| `item.started` | 新操作开始 | `item: {id, type, details}` |
| `item.updated` | 操作状态更新 | `item: {id, type, details}` |
| `item.completed` | 操作完成/失败 | `item: {id, type, details}` |
| `error` | 不可恢复错误 | `message` |

### ThreadItem 详细类型

每种 `ThreadItem` 都有 `type` 字段区分：

| 类型 `type` | 说明 | 关键字段 |
|-----------|------|----------|
| `agent_message` | 模型回复文本 | `text` |
| `reasoning` | 推理摘要 | `text` |
| `command_execution` | 命令执行 | `command`, `aggregated_output`, `exit_code`, `status` |
| `file_change` | 文件变更 | `changes: [{path, kind}]`, `status` |
| `mcp_tool_call` | MCP 工具调用 | `server`, `tool`, `arguments`, `result`, `error`, `status` |
| `collab_tool_call` | 协作工具调用 | `tool`, `sender_thread_id`, `receiver_thread_ids`, `status` |
| `web_search` | 网络搜索 | `id`, `query`, `action` |
| `todo_list` | 待办事项列表 | `items: [{text, completed}]` |
| `error` | 非致命错误 | `message` |

## 代码实现分析

### 触发入口

**文件：** `exec/src/lib.rs:575-582`

```rust
let mut event_processor: Box<dyn EventProcessor> = match json_mode {
    true => Box::new(EventProcessorWithJsonOutput::new(last_message_file.clone())),
    _ => Box::new(EventProcessorWithHumanOutput::create_with_ansi(...)),
};
```

### JSONL 输出核心

**文件：** `exec/src/event_processor_with_jsonl_output.rs:103-115`

```rust
fn emit(&self, event: ThreadEvent) {
    println!("{}", serde_json::to_string(&event).unwrap_or_else(|err| {
        json!({ "type": "error", "message": format!("failed to serialize exec json event: {err}") }).to_string()
    }));
}
```

### CLI 标志定义

**文件：** `exec/src/cli.rs:59-66`

```rust
#[arg(
    long = "json",
    alias = "experimental-json",
    default_value_t = false,
    global = true
)]
pub json: bool,
```

## 事件流示例

```json
{"type":"thread.started","thread_id":"thread_12345"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_0","type":"reasoning","details":{"text":"分析需求..."}}}
{"type":"item.completed","item":{"id":"item_0","type":"reasoning","details":{"text":"分析完成"}}}
{"type":"item.started","item":{"id":"item_1","type":"command_execution","details":{"command":"ls -la","status":"in_progress"}}}
{"type":"item.completed","item":{"id":"item_1","type":"command_execution","details":{"command":"ls -la","aggregated_output":"total 24","exit_code":0,"status":"completed"}}}
{"type":"turn.completed","usage":{"input_tokens":150,"cached_input_tokens":0,"output_tokens":75,"reasoning_output_tokens":50}}
```

## 主要数据结构

### ThreadStartedEvent
```rust
struct ThreadStartedEvent {
    thread_id: String  // 用于恢复线程
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
    }
}
```

### CommandExecutionItem
```rust
struct CommandExecutionItem {
    command: String,
    aggregated_output: String,
    exit_code: Option<i32>,
    status: CommandExecutionStatus  // InProgress | Completed | Failed | Declined
}
```

## 使用场景

### 1. 脚本集成
```bash
#!/bin/bash
OUTPUT=$(codex exec --json "生成 Python API" | jq -r '.item[] | select(.type == "agent_message") | .details.text')
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

## 技术细节

### Item ID 映射机制
- 内部 raw ID → `item_0`, `item_1` 等稳定 ID
- 使用 `HashMap<String, String>` 进行映射
- 保证跨事件的 item 引用一致性

### 状态转换
- `item.started` → `item.updated` → `item.completed`
- `turn.started` → 多个 item → `turn.completed`/`turn.failed`
- 错误时发送 `error` 事件

### 输出隔离
- `#![deny(clippy::print_stdout)]` 强制 stdout 只输出 JSONL
- 所有日志使用 `eprintln!` 输出到 stderr
- 便于流水线处理：`codex exec --json ... 2>/dev/null`

## 相关文件位置

- **CLI 定义：** `exec/src/cli.rs:60`
- **事件类型：** `exec/src/exec_events.rs`
- **JSON 处理器：** `exec/src/event_processor_with_jsonl_output.rs`
- **主循环：** `exec/src/lib.rs:575-582`, `exec/src/lib.rs:833-924`

## 版本信息

当前实现基于 Codex v1.0+，`--experimental-json` 别名已合并到 `--json`。