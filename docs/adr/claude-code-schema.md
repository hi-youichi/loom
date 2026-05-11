---
sidebar_position: 2
title: "ADR: claude-code-schema"
description: "claude-code-schema Crate 设计"
---

# ADR: claude-code-schema Crate

- **状态**：提议中
- **日期**：2025-08-19
- **关联**：`stream-event` crate、Loom headless 集成

## 背景

Loom 需要以 Claude Code CLI 兼容的 JSON 格式对外暴露 Agent 输出，供第三方客户端（如 IDE 插件、自定义 UI）消费。

Claude Code CLI 提供三种 `--output-format`：

| 格式 | 行为 | 用途 |
|------|------|------|
| `text` | 纯文本，默认 | 人读 |
| `json` | 单个 JSON 对象，完成后输出 | 脚本 |
| `stream-json` | NDJSON 事件流，实时 | UI/管道 |

目前没有官方 JSON Schema 定义。社区逆向整理了完整协议（见 [claude-code-parser](https://github.com/udhaykumarbala/claude-code-parser)、[agentmastered stream protocol](https://agentmastered.com/extending-claude/stream-protocol/)）。

本项目需要一个 Rust crate 来类型安全地生成和解析这些输出，供 Loom headless 模式和第三方客户端集成使用。

> **注意**：Loom 仅使用 Claude Code JSON schema 作为 **输出** 格式。Loom 的 Agent 输入不使用 Claude Code 的 `--input-format stream-json` 协议，而是使用 Loom 自身的输入接口。

## 目标

1. **类型安全** — 覆盖所有已知事件类型，编译期保证字段存在
2. **零业务依赖** — 仅依赖 `serde` + `serde_json`，与 `stream-event` crate 一致
3. **双向** — 支持反序列化（解析 CLI 输出）和序列化（测试、转发）
4. **向后兼容** — 未知字段不报错，方便 CLI 迭代

## Crate 结构

```
claude-code-schema/
├── Cargo.toml
└── src/
    ├── lib.rs          # 模块声明 + pub use
    ├── envelope.rs     # --output-format json 最终结果信封
    ├── event.rs        # stream-json NDJSON 顶层事件
    ├── message.rs      # message / content block 共享子类型
    ├── usage.rs        # token 用量、费用相关类型
    └── parse.rs        # NDJSON 行解析 + async reader
```

## 类型设计

### 1. ResultEnvelope（`--output-format json` 完整输出）

对应 `--output-format json` 的单个 JSON 对象，也是 `stream-json` 终态 `type: "result"` 事件。

```rust
/// `--output-format json` 的完整输出信封。
/// 同时用于 stream-json 终态事件（`type: "result"`）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultEnvelope {
    #[serde(default)]
    pub type_: String,                          // 固定 "result"
    pub subtype: ResultSubtype,
    pub result: String,                         // 最终文本回复
    pub session_id: String,
    pub total_cost_usd: f64,
    pub is_error: bool,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub duration_api_ms: Option<u64>,
    #[serde(default)]
    pub num_turns: Option<u32>,
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_usage: Option<BTreeMap<String, Usage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultSubtype {
    Success,
    Error,
    ErrorMaxBudgetUsd,
    ErrorMaxTurns,
    #[serde(other)]
    Unknown(String),
}
```

### 2. StreamJsonEvent（stream-json NDJSON 顶层事件）

每行一个事件，通过 `type` 字段区分。

```rust
/// stream-json 模式下每行 NDJSON 事件的顶层类型。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamJsonEvent {
    /// 会话初始化，第一个事件。
    System {
        subtype: Option<String>,
        #[serde(default)]
        tools: Vec<String>,
        #[serde(default)]
        mcp_servers: Vec<McpServer>,
        #[serde(default)]
        model: Option<String>,
        session_id: String,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        permission_mode: Option<String>,
    },

    /// 模型响应完成事件（非流式，含完整 message）。
    Assistant {
        session_id: String,
        message: Message,
    },

    /// 用户/工具结果事件。
    User {
        session_id: String,
        message: Message,
    },

    /// API 流式事件（token 级别），需 `--include-partial-messages`。
    StreamEvent {
        session_id: String,
        event: ApiStreamEvent,
        #[serde(default)]
        parent_tool_use_id: Option<String>,
        #[serde(default)]
        uuid: Option<String>,
    },

    /// API 限流事件。
    RateLimitEvent {
        #[serde(default)]
        rate_limit_info: Option<RateLimitInfo>,
    },

    /// 终态事件 — 复用 ResultEnvelope。
    Result(ResultEnvelope),
}
```

> **注意**：`StreamJsonEvent::Result` 使用 `#[serde(untagged)]` 内部变体，因为 `ResultEnvelope` 自身包含 `type` 字段。实现时需通过自定义 `Deserialize` 或 `#[serde(flatten)]` 处理。

### 3. ApiStreamEvent（stream_event 内层事件）

```rust
/// `stream_event` 内层事件，对应 Anthropic Messages API 的 SSE 事件。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiStreamEvent {
    MessageStart {
        message: MessageStartPayload,
    },
    ContentBlockStart {
        index: u32,
        content_block: ContentBlock,
    },
    ContentBlockDelta {
        index: u32,
        delta: ContentDelta,
    },
    ContentBlockStop {
        index: u32,
    },
    MessageDelta {
        delta: MessageDeltaPayload,
        usage: Option<DeltaUsage>,
    },
    MessageStop,
}
```

### 4. 共享子类型

#### Message（`message.rs`）

```rust
/// Claude API 消息对象。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub role: String,
    #[serde(default)]
    pub model: Option<String>,
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<Value>,
}
```

#### ContentBlock（`message.rs`）

```rust
/// 消息内的内容块。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(default)]
        is_error: Option<bool>,
    },
    Thinking {
        thinking: String,
    },
}
```

#### ContentDelta（`message.rs`）

```rust
/// 流式内容增量。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    ThinkingDelta {
        thinking: String,
    },
}
```

#### Usage（`usage.rs`）

```rust
/// Token 用量统计。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    pub server_tool_use: Option<Value>,
    #[serde(default)]
    pub service_tier: Option<String>,
}

/// API 限流信息。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitInfo {
    #[serde(default)]
    pub requests_limit: Option<u32>,
    #[serde(default)]
    pub requests_remaining: Option<u32>,
    #[serde(default)]
    pub requests_reset: Option<String>,
    #[serde(default)]
    pub tokens_limit: Option<u32>,
    #[serde(default)]
    pub tokens_remaining: Option<u32>,
    #[serde(default)]
    pub tokens_reset: Option<String>,
    #[serde(default)]
    pub retry_after_ms: Option<u64>,
}

/// MCP 服务器状态。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    #[serde(default)]
    pub status: Option<String>,
}
```

### 5. 解析器（`parse.rs`）

```rust
/// 从 NDJSON 文本行解析为 StreamJsonEvent。
/// 自动忽略空行和不可解析行（如日志噪声）。
pub fn parse_line(line: &str) -> Result<Option<StreamJsonEvent>, ParseError>;

/// 异步 NDJSON 流读取器。
/// 从实现 `AsyncBufRead` 的源逐行读取并解析。
pub struct NdjsonReader<R> { /* ... */ }

impl<R: AsyncBufRead> NdjsonReader<R> {
    pub fn new(reader: R) -> Self;
    /// 读取下一个事件。返回 None 表示 EOF。
    pub async fn next_event(&mut self) -> Result<Option<StreamJsonEvent>, ParseError>;
}
```

## 序列化策略

| 问题 | 策略 |
|------|------|
| CLI 新增未知字段 | 所有 struct 使用 `#[serde(deny_unknown_fields)]` **不启用**，默认忽略 |
| 枚举未知变体 | `ResultSubtype`、`ContentBlock` 等关键枚举加 `#[serde(other)]` 兜底 |
| 可选字段 | 统一 `Option<T>` + `#[serde(default)]` |
| 跳过空值 | 序列化时 `#[serde(skip_serializing_if = "Option::is_none")]` |
| `StreamJsonEvent::Result` 冲突 | 自定义 `Deserialize` 或用 `#[serde(untagged)]` + 内部 `type` 字段匹配 |

## 与现有 Crate 的关系

```
┌─────────────────────────────────────────────────────┐
│                   Loom Headless                      │
│  Agent → StreamEvent → backward() → StreamJsonEvent │
│                                     ──► NDJSON 输出  │
└────────────┬────────────────────────┬───────────────┘
             │                        │
             ▼                        ▼
┌─────────────────────┐  ┌──────────────────────────┐
│  claude-code-schema  │  │      stream-event         │
│  (生成 CLI 输出)      │  │  (Loom 内部流协议)         │
│  serde + serde_json  │  │  serde + serde_json       │
└─────────────────────┘  └──────────────────────────┘
```

- `claude-code-schema`：面向 Claude Code CLI 的外部输出协议
- `stream-event`：面向 Loom Agent 内部协议
- 两者互相独立，不互相依赖
- Loom Headless 通过 `compat-claude-code` 适配层将内部 `stream-event` 转换为 `claude-code-schema` 格式输出

## 事件流时序

### 典型 stream-json 会话

```
1. system (init)           ← 会话开始，含 tools、model
2. stream_event            │ message_start
   └─ message_start        │
3. stream_event            │ content_block_start (text)
   └─ content_block_start  │
4. stream_event            │ content_block_delta (text_delta × N)
   └─ content_block_delta  │  ← 实时文本流
   └─ ...                  │
5. stream_event            │ content_block_stop
   └─ content_block_stop   │
6. stream_event            │ message_delta (stop_reason, usage)
   └─ message_delta        │
7. stream_event            │ message_stop
   └─ message_stop         │
8. result (success)        ← 终态，含 result、cost、usage
```

### 使用工具时的流

```
1. system (init)
2. assistant               ← 含 tool_use content block
3. user                    ← 含 tool_result content block
4. assistant               ← 模型基于工具结果继续
   ... (重复 2-4)
N. result (success)
```

### API 重试

```
1. system (init)
2. system (api_retry)      ← 含 attempt、max_retries、error
   ... (可能多次)
3. result (success/error)
```

## Cargo.toml

```toml
[package]
name = "claude-code-schema"
version.workspace = true
edition.workspace = true
description = "Typed Rust schema for Claude Code CLI JSON output (json + stream-json)"
license.workspace = true
authors.workspace = true

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.0", features = ["io-util"], optional = true }

[features]
default = ["async"]
async = ["tokio"]
```

## 测试策略

1. **快照测试** — 从真实 CLI 输出捕获 NDJSON，作为 `tests/fixtures/` 下的 `.jsonl` 文件
2. **往返测试** — `deserialize → serialize` 保证往返一致性
3. **边界测试** — 空 `content`、缺省字段、未知 `type` 值
4. **集成测试** — `NdjsonReader` 读取多行 fixture 并验证事件序列

```
tests/
├── fixtures/
│   ├── simple_text.jsonl
│   ├── tool_use.jsonl
│   ├── error_max_budget.jsonl
│   └── structured_output.json
├── deserialize_test.rs
├── roundtrip_test.rs
└── ndjson_reader_test.rs
```

## 实现步骤

1. 创建 crate 骨架 + `Cargo.toml`
2. 实现 `usage.rs`（无依赖的基础类型）
3. 实现 `message.rs`（ContentBlock、ContentDelta、Message）
4. 实现 `envelope.rs`（ResultEnvelope、ResultSubtype）
5. 实现 `event.rs`（StreamJsonEvent、ApiStreamEvent）
6. 实现 `parse.rs`（parse_line + NdjsonReader）
7. 添加 fixture + 测试
8. 集成到 Loom headless 输出模式

## 参考

- [Claude Code Headless Docs](https://code.claude.com/docs/en/headless)
- [CLI Reference](https://code.claude.com/docs/en/cli-reference)
- [claude-code-parser (TypeScript)](https://github.com/udhaykumarbala/claude-code-parser)
- [AgentMastered Stream Protocol](https://agentmastered.com/extending-claude/stream-protocol/)
- [GitHub Issue #24596](https://github.com/anthropics/claude-code/issues/24596) — 请求官方 schema 文档
