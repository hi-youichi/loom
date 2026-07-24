# 字段 Gap 补齐开发方案

> 返回 [README.md](README.md) · [TODO.md](TODO.md)
> 依据：[附录 A](appendix-a-opencode-v2-schema.md) §A.8

## G1 — Token 结构对齐（P1）

### 目标

`Usage` 结构体从 OpenAI 风格改为 OpenCode v2 风格。

### 当前

```rust
// foundation/stream-event/src/types/stream_event.rs:7
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_tokens: Option<u32>,
}
```

translator 输出（`translator.rs:177`）：
```json
"tokens": { "prompt": 100, "completion": 50, "total": 150, "cached": 10 }
```

### 改造后

```rust
pub struct Usage {
    pub input: u32,
    pub output: u32,
    pub reasoning: Option<u32>,
    pub cache_read: Option<u32>,
    pub cache_write: Option<u32>,
}
```

translator 输出：
```json
"tokens": { "input": 100, "output": 50, "reasoning": 30, "cache": { "read": 10, "write": 5 } }
```

### 影响文件

| 文件 | 行号 | 改动 |
|---|---|---|
| `foundation/stream-event/src/types/stream_event.rs` | L7-13 | 替换 `Usage` 结构体字段 |
| `foundation/stream-event/src/wire/convert.rs` | L86-87, L468-469, L531-532, L872-873, L879-880 | 更新 `Usage` 构造和断言 |
| `foundation/stream-event/src/wire/protocol.rs` | L59-60, L364-365, L370-371 | 更新 wire 协议字段名 |
| `foundation/stream-event/src/wire/envelope.rs` | L211 | 更新 JSON 断言 |
| `agent/agent-core/src/agent/react/think_node.rs` | L183-194 | 从 `LlmUsage` 构造新 `Usage`：`input = prompt_tokens`, `output = completion_tokens`, `reasoning = completion_tokens_details.reasoning_tokens`, `cache_read = prompt_tokens_details.cached_tokens`, `cache_write = None` |
| `agent/agent-core/src/agent/agent.rs` | L45-51, L157-160, L280-319 | 更新 `AgentUsage` 和 `map_usage_event` |
| `apps/server/src/translator.rs` | L177-181, L330-331, L1319-1351, L1404-1466 | 更新 step-finish part tokens 字段名 + 测试 |
| `apps/cli/src/display/event_handler.rs` | — | 更新 `Usage` 字段引用 |
| `apps/cli/src/run/agent.rs` | — | 同上 |

### 数据来源映射

```
LlmUsage (provider 原始)
├── prompt_tokens               → Usage.input
├── completion_tokens           → Usage.output
├── completion_tokens_details
│   └── reasoning_tokens        → Usage.reasoning
└── prompt_tokens_details
    └── cached_tokens           → Usage.cache_read
                                  Usage.cache_write = None (当前 provider 不返回)
```

## G2 — Delta 增量事件（P1）

### 目标

在 `message.part.updated`（累积文本）之外，额外发送 `message.part.delta`（增量文本），供前端按需消费。

### 当前

`append_to_part`（`translator.rs:293`）只发送 `message.part.updated`（完整 part）。

### 改造后

`append_to_part` 在 emit `message.part.updated` 后追加 emit `message.part.delta`：

```rust
fn append_to_part(...) {
    // ... 现有 state.parts 更新 + emit "message.part.updated" ...

    // 追加增量事件
    emit(
        state,
        "message.part.delta",
        json!({
            "sessionID": session_id,
            "messageID": assistant_msg_id,
            "partID": part_id,
            "field": "text",
            "delta": content,
            "time": chrono::Utc::now().timestamp_millis(),
        }),
    );
}
```

### 影响文件

| 文件 | 行号 | 改动 |
|---|---|---|
| `apps/server/src/translator.rs` | L310-320 (`append_to_part`) | 追加 `message.part.delta` emit |
| `apps/server/src/translator.rs` | 测试 | 新增断言 `message.part.delta` 事件 |

### 向后兼容

前端不监听 `message.part.delta` 则自动忽略，仍从 `message.part.updated` 获取累积文本。不影响现有行为。

## G3 — step-start 携带 agent / model（P2）

### 目标

`TurnStart` 携带 agent 和 model 信息，translator 在 step-start part 中输出。

### 当前

```rust
// TurnStart 无字段
StreamEvent::TurnStart
```

### 改造后

```rust
StreamEvent::TurnStart {
    agent: String,
    model: String,
}
```

### 影响文件

| 文件 | 行号 | 改动 |
|---|---|---|
| `foundation/stream-event/src/types/stream_event.rs` | TurnStart 变体 | 追加 `agent`, `model` 字段 |
| `agent/agent-core/src/agent/react/think_node.rs` | L216 | 发射时携带 `self.provider.provider_name()` 和 `model_config.model_id` |
| `apps/server/src/translator.rs` | L150-162 | step-start part 追加 `"agent"` 和 `"model"` 字段 |
| `apps/server/src/translator.rs` | 测试 | 更新 TurnStart 构造 |
| `apps/cli/src/` | — | 更新 match arm |
| `foundation/stream-event/src/wire/convert.rs` | — | 更新 wire 转换 |

### 数据来源

- `agent`: `self.provider.provider_name()`（如 `"minimax-cn-coding-plan"`）
- `model`: `model_config.model_id`（如 `"MiniMax-M3"`），或从 `resolve_client` 解析结果获取

## G4 — step-finish 携带 cost（P2）

### 目标

`TurnFinish` 携带 cost 字段（估算 API 费用）。

### 当前

```rust
StreamEvent::TurnFinish { reason: String, usage: Usage }
```

### 改造后

```rust
StreamEvent::TurnFinish { reason: String, usage: Usage, cost: Option<f64> }
```

### 影响文件

| 文件 | 行号 | 改动 |
|---|---|---|
| `foundation/stream-event/src/types/stream_event.rs` | TurnFinish 变体 | 追加 `cost: Option<f64>` |
| `agent/agent-core/src/agent/react/think_node.rs` | L187-195 | 传入 `cost`（暂设 `None`，后续按 model pricing 计算） |
| `apps/server/src/translator.rs` | L164-185 | step-finish part 追加 `"cost"` 字段 |
| `foundation/stream-event/src/wire/` | — | 更新转换 |
| `apps/cli/src/` | — | 更新 match arm |

### 说明

cost 计算需要 model pricing 表（每百万 token 价格），当前无此基础设施。暂时传 `None`，后续 X6 snapshot 机制或 pricing 配置落地后补齐。

## G5 — BlockEnd 携带完整文本（P2）

### 目标

`TextBlockEnd` / `ReasoningBlockEnd` 携带完整累积文本，使事件可重放（与 OpenCode v2 `text.ended` / `reasoning.ended` 对齐）。

### 当前

```rust
TextBlockEnd { metadata: StreamMetadata }
ReasoningBlockEnd { id: String, metadata: StreamMetadata }
```

### 改造后

```rust
TextBlockEnd { text: String, metadata: StreamMetadata }
ReasoningBlockEnd { id: String, text: String, metadata: StreamMetadata }
```

### 影响文件

| 文件 | 行号 | 改动 |
|---|---|---|
| `foundation/stream-event/src/types/stream_event.rs` | BlockEnd 变体 | 追加 `text: String` |
| `foundation/stream-event/src/block_tracker.rs` | `close_current` | 从 tracker 累积的文本传入 End 事件 |
| `apps/server/src/translator.rs` | L110-112, L147-148 | finalize 时使用 End 携带的 `text`（而非从 state.parts 读） |
| `foundation/stream-event/src/wire/` | — | 更新转换 |
| `apps/cli/src/` | — | 更新 match arm |

### BlockTracker 改动

```rust
struct BlockTracker<S> {
    active: ActiveBlockKind,
    reasoning_seq: usize,
    text_buf: String,          // 新增：累积当前 text block 的文本
    reasoning_buf: String,     // 新增：累积当前 reasoning block 的文本
}

fn on_text_delta(&mut self, text: &str, metadata: &StreamMetadata) -> Vec<StreamEvent<S>> {
    // ... existing logic ...
    self.text_buf.push_str(text);   // 新增
    events.push(StreamEvent::TextDelta { content: text.to_string(), metadata: ... });
}

fn close_current(&mut self, metadata: &StreamMetadata) -> Vec<StreamEvent<S>> {
    match std::mem::replace(&mut self.active, ActiveBlockKind::None) {
        ActiveBlockKind::Text => {
            let text = std::mem::take(&mut self.text_buf);
            events.push(StreamEvent::TextBlockEnd { text, metadata: ... });
        }
        ActiveBlockKind::Reasoning { id } => {
            let text = std::mem::take(&mut self.reasoning_buf);
            events.push(StreamEvent::ReasoningBlockEnd { id, text, metadata: ... });
        }
        ActiveBlockKind::None => {}
    }
}
```

## 实施顺序

```
G1 (Token 结构)  ─── 独立，不影响 G2-G5
G2 (Delta 事件)  ─── 独立，不影响 G1/G3-G5
G3 (agent/model) ─── 改 TurnStart，依赖 G1（同文件）
G4 (cost)        ─── 改 TurnFinish，依赖 G1（同文件）
G5 (BlockEnd)    ─── 改 BlockTracker + translator，独立

推荐执行：G1 → G5 → G2 → G3+G4（合并改 TurnStart/TurnFinish）
```
