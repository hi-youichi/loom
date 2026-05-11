---
sidebar_position: 3
title: "session cat — 会话回放"
description: "使用 loom session cat 命令回放历史会话，支持 Codex 兼容的 NDJSON 事件流输出和文本摘要"
---

# session cat — 会话回放

`loom session cat` 从 checkpoint 重建历史会话的事件流，输出 Codex 兼容的 NDJSON 或文本摘要。

## 基本用法

```bash
# 文本摘要（默认）
loom session cat session-13f229ef-1086-401b-960f-441ef4634087

# Codex 兼容 NDJSON
loom --json session cat session-13f229ef-1086-401b-960f-441ef4634087
```

> `--json` 是顶层标志，必须放在子命令之前。

## 查看可用会话

```bash
loom session list
loom --json session list
```

## 输出格式

### 文本摘要

```
Session: session-13f229ef-1086-401b-960f-441ef4634087
════════════════════════════════════════════════════════════

────────────────────────────────────────
  [think] 分析需求，设计架构方案...
  [item_1] loom/invoke_agent (completed)
  [reply] 根据分析结果，建议采用以下方案...
  [item_3] loom/read (completed)
  [item_4] loom/batch (completed)
  [usage] in:77754 out:2005 cached:77248 reasoning:0
```

图标含义：

| 标记 | 含义 |
|------|------|
| `[think]` | 模型推理摘要 |
| `[reply]` | 模型回复文本 |
| `[item_N]` | 工具调用（server/tool） |
| `[usage]` | token 使用统计 |

### NDJSON 事件流

每行一个 JSON 对象，与 `codex exec --json` 格式兼容：

```json
{"type":"thread_started","thread_id":"session-abc"}
{"type":"turn_started"}
{"type":"item_started","item":{"id":"item_0","type":"reasoning","text":"分析需求..."}}
{"type":"item_completed","item":{"id":"item_0","type":"reasoning","text":"分析需求..."}}
{"type":"item_started","item":{"id":"item_1","type":"mcp_tool_call","server":"loom","tool":"read","arguments":{"path":"src/main.rs"},"result":null,"error":null,"status":"in_progress"}}
{"type":"item_completed","item":{"id":"item_1","type":"mcp_tool_call","server":"loom","tool":"read","arguments":{"path":"src/main.rs"},"result":{"content":[{"type":"text","text":"..."}],"structured_content":null},"error":null,"status":"completed"}}
{"type":"item_started","item":{"id":"item_2","type":"agent_message","text":"已阅读文件..."}}
{"type":"item_completed","item":{"id":"item_2","type":"agent_message","text":"已阅读文件..."}}
{"type":"turn_completed","usage":{"input_tokens":500,"cached_input_tokens":0,"output_tokens":200,"reasoning_output_tokens":50}}
```

## 事件结构

### 事件类型

| 事件 | 说明 | 出现时机 |
|------|------|----------|
| `thread_started` | 会话开始 | 流首行 |
| `turn_started` | 轮次开始 | 每个 checkpoint 区间 |
| `turn_completed` | 轮次完成 | 携带 usage |
| `turn.failed` | 轮次失败 | LLM 错误（Phase 2） |
| `item_started` | 操作开始 | reasoning / tool_call / agent_message |
| `item_completed` | 操作完成 | 同上 |
| `error` | 不可恢复错误 | 流终止（Phase 2） |

### ThreadItem 类型映射

Loom 内部消息到 Codex ThreadItem 的映射关系：

| Loom 消息 | Codex Item 类型 | 说明 |
|-----------|----------------|------|
| `Assistant.reasoning_content` | `reasoning` | 模型推理过程 |
| `Assistant.tool_calls` (bash) | `command_execution` | shell 命令执行 |
| `Assistant.tool_calls` (其他) | `mcp_tool_call` | 工具调用（read/grep/write 等） |
| `Assistant.content` | `agent_message` | 模型回复文本 |

### 工具调用详情

**command_execution**（bash 工具）：

```json
{
  "id": "item_3",
  "type": "command_execution",
  "command": "cargo build --package cli 2>&1",
  "aggregated_output": "Tool bash result:\n   Compiling cli...",
  "exit_code": 0,
  "status": "completed"
}
```

**mcp_tool_call**（其他工具）：

```json
{
  "id": "item_4",
  "type": "mcp_tool_call",
  "server": "loom",
  "tool": "read",
  "arguments": {"path": "src/main.rs"},
  "result": {
    "content": [{"type": "text", "text": "file content here"}],
    "structured_content": null
  },
  "error": null,
  "status": "completed"
}
```

## 实现原理

`session cat` 从 SQLite `checkpoints` 表加载指定会话的所有 checkpoint，按时间升序排列后逐个比较差异：

1. 加载所有 checkpoint，解码为 `ReActState`
2. 相邻 checkpoint 按非系统消息数量差值提取新增消息
3. 跳过无 Assistant 消息的 checkpoint（纯初始化状态）
4. 将 Assistant 消息中的 reasoning / tool_calls / content 映射为 Codex ThreadItem
5. 从消息流中的 Tool 消息提取工具执行结果
6. 按 `started → completed` 模式输出事件

## 当前限制

| 限制 | 说明 | 解决方案 |
|------|------|----------|
| 无增量 delta | 不输出 `CommandExecutionOutputDelta` 等流式增量 | Phase 2: event_log 表 |
| 无 LLM 错误详情 | `turn.failed` 和顶层 `error` 不可从 checkpoint 重建 | Phase 2: 运行时事件捕获 |
| 工具结果含包装文本 | output 包含 `Tool bash result:` 前缀 | 原始数据如此，消费端可自行去除 |
| Turn 粒度受限于 checkpoint | 一个 checkpoint 可能包含多个 LLM 轮次 | Phase 2: 更细粒度的事件记录 |

## 典型用法

### 管道处理

```bash
# 提取所有模型回复
loom --json session cat session-abc | jq -r 'select(.type=="item_completed" and .item.type=="agent_message") | .item.text'

# 提取所有执行的命令
loom --json session cat session-abc | jq -r 'select(.type=="item_completed" and .item.type=="command_execution") | "\(.item.command) → exit:\(.item.exit_code)"'

# 统计 token 使用
loom --json session cat session-abc | jq -r 'select(.type=="turn_completed") | .usage'

# 检查是否有失败的工具调用
loom --json session cat session-abc | jq 'select(.item.status=="failed")'
```

### 会话对比

```bash
# 比较两个会话的工具调用差异
diff <(loom --json session cat session-1 | jq -r '.item.tool // empty' | sort) \
     <(loom --json session cat session-2 | jq -r '.item.tool // empty' | sort)
```

## 待决事项

### 1. 降级策略：未完成的 turn 是否 emit turn.failed

文档定义了降级策略：最后一个 checkpoint 无 `usage` 且无 Assistant 回复时，emit `turn.failed`。当前 builder 未实现。

**选项**：
- A）实现降级：检测最后一个 checkpoint 状态，条件性输出 `turn.failed`
- B）不实现：保持现状，最后一个 turn 始终输出 `turn.completed`（usage 可能为 0）

### 2. 工具结果包装文本清洗

当前 output 包含 Loom 的包装前缀（如 `Tool bash result:`、`Tool read result:`）。这是原始数据格式。

**选项**：
- A）Builder 清洗：去除 `Tool xxx result:\n` 前缀，输出纯净内容
- B）不清洗：保持原始数据，消费端自行处理

### 3. Turn 粒度：按 checkpoint vs 按 Assistant 消息拆分

当前 1 个 checkpoint = 1 个 turn。Loom 的 ReAct 循环可能在一个 checkpoint 里跑多轮 LLM 调用（工具调用 → 再调用），导致单个 turn 含几十个 item。

**选项**：
- A）保持现状：1 checkpoint = 1 turn，简单直接
- B）按 Assistant 消息拆分：每个 Assistant 消息（含其对应的 Tool 消息）作为独立 turn，需推断 usage
- C）按 tool_calls 阶段拆分：同一 Assistant 消息中的 reasoning + tool_calls + 后续 Tool 结果 + 最终回复 = 1 turn

### 4. Item ID 稳定性

当前按顺序分配 `item_0`、`item_1`...，同一 session 多次 cat 结果一致。但如果中间某个 checkpoint 被清理或新增，ID 会整体偏移。

**选项**：
- A）保持顺序 ID：简单，但依赖 checkpoint 完整性
- B）基于内容哈希：对 (tool_name + arguments + result) 哈希生成确定性 ID，如 `item_a3f2b1`
- C）基于 checkpoint step + 序号：`item_{step}_{seq}`，与 checkpoint 位置绑定

### 5. file_change 类型映射

当前所有非 bash 工具统一映射为 `mcp_tool_call`。但 Loom 的 write/edit/diff 等工具实际产生了文件变更，Codex 协议有专门的 `file_change` item 类型。

**选项**：
- A）保持现状：所有工具统一 `mcp_tool_call`，信息不丢失但语义不够精确
- B）按工具名映射：`write`/`edit`/`diff` → `file_change`，需解析工具参数提取 path 和 kind
- C）从 Tool result 反推：检查结果中是否包含 diff/patch 信息，自动判断是否为文件变更

## 相关文档

- [Codex 事件协议字段参考](/docs/reference/codex-event-protocol) — 完整的事件类型和字段定义
- [Codex 异常处理](/docs/reference/codex-error-handling) — 错误事件的处理策略
- [CLI JSON 流式输出](/docs/deployment/cli-json-output) — 运行时的 `--json` 事件流
