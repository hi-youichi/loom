---
sidebar_position: 3
title: "Claude Code 兼容层设计"
description: "第三方兼容层：Claude Code CLI JSON 协议的消费、适配和服务端实现"
---

# Claude Code 兼容层设计

- **状态**：提议中
- **日期**：2025-08-19
- **关联**：`docs/adr/claude-code-schema.md`、`docs/reference/claude-code-json-protocol.md`、`stream-event` crate

## 1. 概述

本文档定义 Loom 与 Claude Code CLI JSON 协议的三层兼容架构：

| 层级 | 名称 | Crate | 职责 |
|------|------|-------|------|
| Level 1 | 消费端 | `claude-code-schema` | 类型安全地解析 Claude Code CLI 输出 |
| Level 2 | 适配层 | `compat-claude-code` | Loom `StreamEvent` → Claude Code `StreamJsonEvent` 单向转换（输出） |
| Level 3 | 服务端 | — | Loom 作为 Claude Code CLI 的替代品对外提供服务 |

### 目标

1. **完整解析** — 覆盖 Claude Code CLI 所有已知 JSON 事件类型（json + stream-json）
2. **单向适配** — 将 Loom `StreamEvent` / `ProtocolEvent` 转换为 Claude Code `StreamJsonEvent`（仅输出方向）
3. **可扩展** — 统一放在 `crates/compat/` 下，未来可加入其他第三方兼容层
4. **零业务依赖** — Schema crate 仅依赖 serde，适配层仅依赖 schema + stream-event

### 非目标

- 不实现 Claude Code CLI 的子进程管理（由调用方负责 spawn / stdin / stdout）
- Level 3 仅设计，不在此阶段实现

## 2. 架构

### 2.1 Crate 依赖关系

```
crates/compat/
├── claude-code-schema/          # Level 1: 纯数据类型
│   └── depends on: serde, serde_json, tokio (optional, async)
│
├── compat-claude-code/          # Level 2: 适配层
│   └── depends on: claude-code-schema, stream-event
│
└── (未来) compat-xxx/           # 其他第三方兼容层
```

### 2.2 数据流

```
Loom Agent ──► StreamEvent<S> ──► convert() ──► StreamJsonEvent ──► NDJSON ──► 客户端
```

### 2.3 与 Workspace 的关系

```toml
# 根 Cargo.toml 新增
[workspace]
members = [
    # ... 现有成员
    "crates/compat/claude-code-schema",
    "crates/compat/compat-claude-code",
]
```

## 3. Level 1: claude-code-schema

> Crate 详细设计见 [ADR: claude-code-schema](../adr/claude-code-schema.md)。

本节仅补充 ADR 未覆盖的要点。

### 3.1 Crate 结构

```
crates/compat/claude-code-schema/
├── Cargo.toml
└── src/
    ├── lib.rs          # 模块声明 + pub use
    ├── envelope.rs     # ResultEnvelope, ResultSubtype
    ├── event.rs        # StreamJsonEvent, ApiStreamEvent
    ├── message.rs      # Message, ContentBlock, ContentDelta, ContentBlockType
    ├── usage.rs        # Usage, ModelUsage, RateLimitInfo, McpServer
    └── parse.rs        # parse_line(), NdjsonReader<R>
```

### 3.2 关键序列化决策

| 问题 | 策略 |
|------|------|
| CLI 新增未知字段 | 不使用 `deny_unknown_fields`，默认忽略 |
| 枚举未知变体 | `ResultSubtype` 等关键枚举加 `#[serde(other)]` 兜底变体 |
| 可选字段 | 统一 `Option<T>` + `#[serde(default)]` |
| 跳过空值 | `#[serde(skip_serializing_if = "Option::is_none")]` |
| 字段命名不一致 | `modelUsage` 内部用 `#[serde(rename_all = "camelCase")]`；顶层用 `snake_case` |
| `StreamJsonEvent::Result` 与 `type: "result"` 冲突 | 自定义 `Deserialize`，先读 `type` 字段再分发 |

### 3.3 NdjsonReader

```rust
pub struct NdjsonReader<R> {
    reader: tokio::io::BufReader<R>,
    line_buf: String,
}

impl<R: tokio::io::AsyncRead + Unpin> NdjsonReader<R> {
    pub fn new(reader: R) -> Self;

    /// 读取并解析下一个事件。自动跳过空行和不可解析行。
    /// 返回 `Ok(None)` 表示 EOF。
    pub async fn next_event(&mut self) -> Result<Option<StreamJsonEvent>, ParseError>;
}
```

### 3.4 Cargo.toml

```toml
[package]
name = "claude-code-schema"
version.workspace = true
edition.workspace = true
description = "Typed Rust schema for Claude Code CLI JSON output (json + stream-json)"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.0", features = ["io-util"], optional = true }

[features]
default = ["async"]
async = ["tokio"]
```

## 4. Level 2: compat-claude-code 适配层

### 4.1 Crate 结构

```
crates/compat/compat-claude-code/
├── Cargo.toml
└── src/
    ├── lib.rs          # 模块声明 + pub use
    ├── convert.rs     # ProtocolEvent → StreamJsonEvent
    └── gaps.rs         # 语义差异定义和处理
```

### 4.2 Cargo.toml

```toml
[package]
name = "compat-claude-code"
version.workspace = true
edition.workspace = true
description = "Adapter: Loom StreamEvent protocol → Claude Code CLI JSON output"

[dependencies]
claude-code-schema = { path = "../claude-code-schema" }
stream-event = { path = "../../../stream-event" }
serde_json = "1.0"
```

### 4.3 Convert: Loom → Claude Code

将 Loom 的 `ProtocolEvent` 转换为 Claude Code 的 `StreamJsonEvent`，用于让 Loom Agent 对外暴露 Claude Code 兼容的 JSON 流。

> **方向**：仅 Loom → Claude Code 输出方向。不实现反向转换（Claude Code → Loom）。

#### 映射表

| Loom ProtocolEvent | Claude Code 事件 | 转换逻辑 |
|---|---|---|
| `NodeEnter { id }` | — | 内部追踪，不直接输出 |
| `NodeExit { id, result }` | — | 内部追踪，不直接输出 |
| `MessageChunk { content, id }` | `stream_event` (content_block_delta: text_delta) | 包装为 stream_event |
| `ThoughtChunk { content, id }` | `stream_event` (content_block_delta: thinking_delta) | 包装为 stream_event |
| `Usage { ... }` | `stream_event` (message_delta) | 包装为 message_delta |
| `Values { state }` | `result (success)` | 终态事件 |
| `Updates { id, state }` | — | 忽略（或映射为 Custom） |
| `Custom { value }` | `stream_event` (content_block_delta: text_delta) | JSON 序列化为文本 |
| `Checkpoint { ... }` | — | 忽略 |
| `ToolCall { call_id, name, arguments }` | `assistant` (tool_use content block) | 或 stream_event content_block_start |
| `ToolStart { ... }` | — | 忽略 |
| `ToolOutput { ... }` | — | 忽略 |
| `ToolEnd { call_id, name, result, is_error }` | `user` (tool_result) | 包装 tool_result |
| `ToolApproval { ... }` | `assistant` (tool_use) + 等待审批 | 需要客户端回传审批结果 |
| `ToolCallChunk { ... }` | `stream_event` (input_json_delta) | 直接映射 |
| `TotExpand { ... }` | `Custom { value }` → `stream_event` | 无直接对应，映射为 Custom |
| `TotEvaluate { ... }` | `Custom { value }` → `stream_event` | 无直接对应，映射为 Custom |
| `TotBacktrack { ... }` | `Custom { value }` → `stream_event` | 无直接对应，映射为 Custom |
| `GotPlan { ... }` | `Custom { value }` → `stream_event` | 无直接对应，映射为 Custom |
| `GotNodeStart { ... }` | `Custom { value }` → `stream_event` | 无直接对应，映射为 Custom |
| `GotNodeComplete { ... }` | `Custom { value }` → `stream_event` | 无直接对应，映射为 Custom |
| `GotNodeFailed { ... }` | `Custom { value }` → `stream_event` | 无直接对应，映射为 Custom |
| `GotExpand { ... }` | `Custom { value }` → `stream_event` | 无直接对应，映射为 Custom |

#### Convert API

```rust
/// 将一个 Loom ProtocolEvent 转换为零或多个 Claude Code StreamJsonEvent。
pub fn convert(event: ProtocolEvent, ctx: &mut ConvertContext) -> Vec<StreamJsonEvent>;

/// Convert 转换的上下文状态。
pub struct ConvertContext {
    pub session_id: String,
    pub current_model: String,
    pub node_stack: Vec<String>,
    /// 是否为流式输出模式（影响 assistant 事件 vs stream_event 的选择）
    pub stream_mode: bool,
}
```

#### 事件流时序示例

Loom ReAct Agent 对外暴露为 Claude Code stream-json 流：

```
1. Custom: { "type": "system", "subtype": "init", "session_id": "...", "model": "...", "tools": [...] }
2. stream_event: content_block_start (text)
3. stream_event: content_block_delta (text_delta) × N    ← MessageChunk
4. stream_event: content_block_stop
5. assistant: { tool_use content block }                  ← ToolCall
6. user: { tool_result content block }                    ← ToolEnd
7. stream_event: content_block_delta (text_delta) × N    ← 后续 MessageChunk
8. result: { "subtype": "success", "result": "...", "total_cost_usd": ... }
```

### 4.5 语义差异（gaps.rs）

两类协议之间的语义差异定义和处理策略：

| 差异 | 处理策略 |
|---|---|
| Loom 有 ToT/GoT 事件 | 映射为 `stream_event` + `Custom` value，客户端按需解析 |
| Claude Code `modelUsage` 用 camelCase | schema crate 内部 `#[serde(rename_all)]` 处理 |
| Loom 有 `ThoughtChunk` | 映射为 `thinking_delta` stream_event |
| Loom 有 `Checkpoint` | 不映射，Loom 自己管理持久化 |
| Loom `Usage` 有 prefill/decode duration | 丢弃 duration 字段，仅映射 token 计数 |

```rust
/// 语义差异处理结果的附带信息。
pub struct GapMetadata {
    /// 被丢弃的原始字段（用于日志/调试）
    pub dropped_fields: Vec<String>,
    /// 降级映射的说明（如 "TotExpand mapped to Custom"）
    pub downgrade_notes: Vec<String>,
}
```

## 5. Level 3: Headless Server（设计，暂不实现）

Loom 作为 Claude Code CLI 的替代品，对外暴露完全兼容的 `--output-format stream-json` 协议。

### 5.1 架构

```
┌──────────────────────────────────────────────────────┐
│                   Loom Headless Server                │
│                                                      │
│  stdin / API ──► Loom Agent Input ──► Loom Agent     │
│  (非 Claude Code 输入协议)                             │
│                                      │               │
│                                 StreamEvent<S>        │
│                                      │               │
│                              convert() 转换          │
│                                      │               │
│                              NdjsonWriter ──► stdout  │
│                                                      │
│  输入: Loom 自有接口（非 --input-format stream-json）   │
│  输出: --output-format stream-json --verbose          │
└──────────────────────────────────────────────────────┘
```

### 5.2 协议兼容矩阵

| Claude Code CLI 标志 | Loom Headless 等价 |
|---|---|
| `-p "query"` | stdin 输入 user message（Loom 自有格式，非 Claude Code 输入协议） |
| `--output-format json` | Agent 完成后输出单个 `ResultEnvelope` |
| `--output-format stream-json` | Agent 运行时实时输出 NDJSON |
| `--resume session_id` | 从 Checkpointer 恢复会话 |
| `--max-turns N` | 配置 Agent max_turns |
| `--max-budget-usd N` | 配置 LlmProvider 预算 |
| `--model MODEL` | 配置 model 名称 |
| `--verbose` | 输出 system (init) 事件 |
| `--include-partial-messages` | 输出 stream_event 事件 |
| `--json-schema SCHEMA` | 配置结构化输出 |

### 5.3 实施路径

1. 实现 `NdjsonWriter`（`compat-claude-code` 的输出端）
2. 实现 input parser（解析 stdin 为 Loom Agent 输入，非 Claude Code 输入协议）
3. 创建 `loom-headless` binary crate
4. 集成测试：用真实 Claude Code 客户端连接 Loom Headless

## 6. 集成点

### 6.1 Loom Headless（Level 2 输出）

Loom 通过 `compat-claude-code` 适配层将内部 `StreamEvent` 转换为 Claude Code 兼容的 `StreamJsonEvent` NDJSON 流输出，供第三方客户端消费。

> **注意**：Loom 仅使用 Claude Code JSON schema 作为输出格式。Loom 的 Agent 输入不使用 Claude Code 的输入协议。

### 6.2 WebSocket Server（Level 2）

```
serve/src/
└── ws.rs    # WebSocket handler
```

改动：
- 新增 `StreamJsonEvent` 序列化输出模式
- 客户端可选择 `protocol=loom`（默认 ProtocolEvent）或 `protocol=claude-code`（StreamJsonEvent）

### 6.3 未来：Loom Headless（Level 3）

- 新 binary: `crates/compat/loom-headless/`
- 替代 Claude Code CLI 的使用场景

## 7. 实施计划

### Phase 1: claude-code-schema crate

1. 创建 `crates/compat/claude-code-schema/` 骨架
2. 实现 `usage.rs`
3. 实现 `message.rs`
4. 实现 `envelope.rs`
5. 实现 `event.rs`
6. 实现 `parse.rs`（`parse_line` + `NdjsonReader`）
7. 添加 fixture + 单元测试 + 往返测试
8. 注册到 workspace `Cargo.toml`

### Phase 2: compat-claude-code crate

1. 创建 `crates/compat/compat-claude-code/` 骨架
2. 实现 `gaps.rs`（语义差异定义）
3. 实现 `convert.rs`（ProtocolEvent → StreamJsonEvent）
4. 添加映射测试（使用 fixture 数据）

### Phase 3: 集成

1. Loom Headless streaming 输出使用 `claude-code-schema`
2. `serve` WebSocket 添加 `protocol=claude-code` 输出模式
3. 集成测试

### Phase 4: Headless Server

1. 实现 `NdjsonWriter`
2. 创建 `loom-headless` binary
3. 端到端集成测试

> **重要**：Loom headless 的输出使用 Claude Code JSON schema 格式，但输入不使用 Claude Code 的 `--input-format stream-json` 协议。输入由 Loom 自身的 Agent 接口处理。

## 参考

- [Claude Code JSON 协议参考](../reference/claude-code-json-protocol.md) — 完整的 CLI JSON 输出协议文档
- [Claude Code JSON Schema 类型详解](../reference/claude-code-schema-types.md) — Schema 核心类型的结构和约束说明
- [ADR: claude-code-schema](../adr/claude-code-schema.md) — Schema crate 的详细类型设计
- [Claude Code Headless Docs](https://code.claude.com/docs/en/headless)
- [Claude Code CLI Reference](https://code.claude.com/docs/en/cli-reference)
- [Loom ProtocolEvent](../../stream-event/src/event.rs) — Loom 内部流事件类型
- [Loom StreamEvent](../../loom/src/stream/stream_event.rs) — Loom Agent 流事件类型
