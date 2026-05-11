---
sidebar_position: 2
title: "Claude Code JSON Schema 类型详解"
description: "Claude Code CLI JSON 输出的核心数据类型说明，从适配层视角描述各类型的用途、结构和约束"
---

# Claude Code JSON Schema 类型详解

> 完整类型定义见 [ADR: claude-code-schema](../adr/claude-code-schema.md)，协议字段说明见 [JSON 协议参考](./claude-code-json-protocol.md)。本文从适配层视角说明关键类型的用途和约束。

适配层 `convert()` 输出的核心类型层次：

```
StreamJsonEvent             ← NDJSON 每行的顶层事件
├── System { ... }          ← 会话初始化（Loom 需构造）
├── Assistant { message }   ← 模型响应（ToolCall / 文本）
├── User { message }        ← 工具结果回传
├── StreamEvent { event }   ← token 级流式增量
├── Result(ResultEnvelope)  ← 终态结果
└── (RateLimitEvent)        ← Loom 不输出，仅解析时使用
```

## 1. StreamJsonEvent（`event.rs`）

NDJSON 流中每行 JSON 的顶层枚举，通过 `type` 字段区分：

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamJsonEvent {
    System { subtype, session_id, tools, model, ... },
    Assistant { session_id, message },
    User { session_id, message, parent_tool_use_id? },
    StreamEvent { session_id, event, parent_tool_use_id?, uuid? },
    RateLimitEvent { rate_limit_info? },
    Result(ResultEnvelope),
}
```

Loom convert 输出时使用的变体：
- **System (init)** — 会话开始时构造，包含 `session_id`、`model`、`tools` 列表
- **Assistant** — 完整模型响应（含 `tool_use` 时使用），对应 Loom 的 `ToolCall` / `ToolApproval`
- **User** — 工具执行结果回传，对应 Loom 的 `ToolEnd`
- **StreamEvent** — token 级流式增量，对应 Loom 的 `MessageChunk` / `ThoughtChunk` / `ToolCallChunk`
- **Result** — 终态事件，对应 Loom 的 `Values`

## 2. ResultEnvelope（`envelope.rs`）

终态事件，`--output-format json` 的完整输出和 `stream-json` 的最后一个事件共享此结构：

```rust
pub struct ResultEnvelope {
    pub type_: String,              // 固定 "result"
    pub subtype: ResultSubtype,     // success / error / error_max_budget_usd / error_max_turns
    pub result: String,             // 最终文本回复
    pub session_id: String,
    pub total_cost_usd: f64,
    pub is_error: bool,
    pub duration_ms: u64,
    pub duration_api_ms: Option<u64>,
    pub num_turns: Option<u32>,
    pub usage: Usage,
    pub model_usage: Option<BTreeMap<String, ModelUsage>>,
    pub structured_output: Option<Value>,
    pub stop_reason: Option<String>,
}
```

关键字段填充来源：
- `total_cost_usd` / `usage` — 从 Loom `Usage` 事件累计
- `result` — 从 Loom `Values { state }` 提取
- `session_id` — 从 `ConvertContext` 获取
- `duration_ms` — 从会话开始计时

## 3. Message 与 ContentBlock（`message.rs`）

`assistant` 和 `user` 事件的 `message` 字段遵循 Anthropic Messages API 格式：

```rust
pub struct Message {
    pub id: Option<String>,
    pub type_: Option<String>,
    pub role: String,               // "assistant" 或 "user"
    pub model: Option<String>,
    pub content: Vec<ContentBlock>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
}
```

ContentBlock 多态枚举（`type` 字段区分）：

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text },
    ToolUse { id, name, input },       // assistant 消息中的工具调用
    ToolResult { tool_use_id, content, is_error? },  // user 消息中的工具结果
    Thinking { thinking },             // 扩展思考内容
}
```

> **多态陷阱**：`ToolResult.content` 在 Claude Code 中可能是 `string` 或 `ContentBlock[]`。`claude-code-schema` 用 `serde_json::Value` 统一处理，适配层构造时统一用字符串。

## 4. ApiStreamEvent（`event.rs`）

`stream_event` 内层事件，对应 Anthropic API 的 SSE 事件类型：

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiStreamEvent {
    MessageStart { message },
    ContentBlockStart { index, content_block },
    ContentBlockDelta { index, delta },
    ContentBlockStop { index },
    MessageDelta { delta, usage? },
    MessageStop,
}
```

适配层输出 `StreamEvent` 时常用内层事件：
- `content_block_delta (text_delta)` — 映射 Loom `MessageChunk`
- `content_block_delta (thinking_delta)` — 映射 Loom `ThoughtChunk`
- `content_block_delta (input_json_delta)` — 映射 Loom `ToolCallChunk`
- `content_block_start (tool_use)` — 映射 Loom `ToolCall`（流式模式）
- `message_delta` — 映射 Loom `Usage`

## 5. 序列化注意事项

| 问题 | 说明 |
|------|------|
| 命名不一致 | 顶层字段 `snake_case`，`modelUsage` 内部 `camelCase`，`rate_limit_info` 内部 `camelCase` |
| 不使用 `deny_unknown_fields` | Claude Code CLI 可能随时新增字段，必须容忍未知字段 |
| `ResultEnvelope` 嵌入 `StreamJsonEvent` | `ResultEnvelope` 自身有 `type: "result"` 字段，需自定义 `Deserialize` 处理 tag 冲突 |
| UUID 生成 | 每个事件需要 `uuid` 字段，Loom 构造时用 `uuid::Uuid::new_v4()` |
| `session_id` 传播 | 所有事件都需包含 `session_id`，从 `ConvertContext` 获取 |

## 参考

- [Claude Code JSON 协议参考](./claude-code-json-protocol.md) — 完整的 CLI JSON 输出协议文档
- [ADR: claude-code-schema](../adr/claude-code-schema.md) — Schema crate 的详细类型设计
- [Claude Code 兼容层设计](../design/claude-code-compat.md) — 适配层整体架构和转换映射
