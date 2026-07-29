# StreamEvent 改造：消除 part 类型推断

[02-part-lifecycle.md](02-part-lifecycle.md) §2.1–§2.5 的 `ActivePart` 推断方案是对 Loom 缺少三段式事件的**补偿**。
根本解法是在 LLM streaming 层注入**显式的 start/end 事件**，使 translator 无需猜测 part 边界。

## 8.2 方案：扩展现有 StreamEvent（不新建类型）

> 开发任务：A1-A7（枚举改造）

现有 `StreamEvent<S>`（`foundation/stream-event/src/types/stream_event.rs`）已经把 tool 生命周期拆成了
`ToolCall → ToolStart → ToolOutput → ToolEnd` 四个变体——text/reasoning 只需要同样模式：

```
// 现有 tool 模式：
ToolCall { ... } → ToolStart { ... } → ToolOutput { ... } → ToolEnd { ... }

// 对称地加 text/reasoning 模式：
TextBlockStart → TextDelta → TextBlockEnd
ReasoningBlockStart → ReasoningDelta → ReasoningBlockEnd
```

**为什么比新建 `LlmStreamEvent` 好**：

| 方面 | 新建 LlmStreamEvent | 扩展现有 StreamEvent（本方案） |
|---|---|---|
| 转换层 | 需要 `LlmStreamEvent → StreamEvent` 适配器 | 无——直接发 StreamEvent |
| 影响面 | 新 crate + 改 agent-core + 改 translator | 仅改 `stream_event.rs` + SSE parser + translator |
| 一致性 | 两套并行的事件类型 | 一套统一类型，tool/text 模式对称 |
| 向后兼容 | `Messages` 和 `LlmStreamEvent` 共存 | 迁移期将 `Messages` 转换为显式 delta，最终删除旧变体 |

## 8.3 改造后的 `StreamEvent<S>`

> 开发任务：A1-A7

下方代码展示改造后的最终形态：`Messages { chunk, metadata }` 被
`TextDelta` / `ReasoningDelta` 替换，并追加 block / turn / error / finish 边界变体。

在现有 `StreamEvent<S>` 枚举中替换并追加：

```rust
// foundation/stream-event/src/types/stream_event.rs

pub enum StreamEvent<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    // ── 保持不变的现有变体 ──
    Values(S),
    Updates { node_id, state, namespace },
    TextDelta {
        content: String,
        metadata: StreamMetadata,
    },
    ReasoningDelta {
        id: String,
        content: String,
        metadata: StreamMetadata,
    },
    Custom(Value),
    Checkpoint(CheckpointEvent<S>),
    TaskStart { node_id, namespace },
    TaskEnd { node_id, result, namespace },
    // ... ToT/GoT 变体 ...
    // Usage 变体删除——usage 由 TurnFinish.usage 携带。
    Usage { prompt_tokens, completion_tokens, total_tokens, cached_tokens, ... },
    ToolCall { call_id, name, arguments },
    ToolStart { call_id, name },
    ToolOutput { call_id, name, content },
    ToolEnd { call_id, name, result, is_error, raw_result },

    // ── 新增：text/reasoning block 生命周期 ──

    /// 文本 block 开始（等价于 OpenCode `text-start`）。
    /// 在首次 text delta 前发出；translator 创建新 text part。
    TextBlockStart {
        metadata: StreamMetadata,
    },

    /// 文本 block 结束（等价于 OpenCode `text-end`）。
    /// 在 text → reasoning 类型翻转、或 ToolCall 到达、或 stream 结束时发出；
    /// translator 收尾当前活跃 text part（设置 time.end）。
    TextBlockEnd {
        metadata: StreamMetadata,
    },

    /// 推理 block 开始（等价于 OpenCode `reasoning-start`）。
    /// `id` 为 provider 返回的 reasoning stream ID（如 Anthropic content_block index），
    /// 无 provider ID 时用 `BlockTracker` 生成的 `"r0"`, `"r1"`, ...。
    ReasoningBlockStart {
        id: String,
        metadata: StreamMetadata,
    },

    /// 推理 block 结束（等价于 OpenCode `reasoning-end`）。
    ReasoningBlockEnd {
        id: String,
        metadata: StreamMetadata,
    },

    // ── 新增：回合生命周期 ──

    /// 单次 LLM 调用开始（等价于 OpenCode `step-start`）。
    /// 在 LLM stream 开始时发出，包裹单次 LLM 请求的全部事件。
    TurnStart,

    /// 单次 LLM 调用结束（等价于 OpenCode `step-finish`）。
    /// 在 LLM stream 结束时立即发出，携带 finish_reason 和 token 用量。
    /// 注意：与 `TaskEnd` 不同——`TurnFinish` 在 LLM 响应结束时即触发，
    /// 而 `TaskEnd` 在整个 node 逻辑（含 tool dispatch）完成后才触发。
    TurnFinish {
        reason: String,
        usage: Usage,
    },

    // ── 新增：缺失事件 ──

    /// SDK 直接报错（独立于 ToolEnd 的错误路径）。
    /// 等价于 OpenCode `tool-error`。
    ToolError {
        call_id: Option<String>,
        error: String,
    },

    /// Provider 级别错误（等价于 OpenCode `provider-error`）。
    ProviderError {
        message: String,
    },

    /// 流正常结束（等价于 OpenCode `finish`）。
    Finish,
}
```

**设计原则**：delta 事件自描述类型，不依赖当前活跃 block 推断。新的 `*Start`/`*End` 变体是
**边界信号**，分别包裹 `TextDelta` 和 `ReasoningDelta`。迁移期可在上游设置适配器，将旧
`Messages` 按 `MessageChunkKind` 转换为显式 delta；所有生产者迁移后删除旧变体。

### 8.3.1 Translator 最小状态

> 开发任务：E1（active_text / active_reasoning）

删除用于类型推断的旧逻辑，但仍需保存事件 ID 到 part ID 的确定性映射：

```rust
active_text: HashMap<String, String>,
active_reasoning: HashMap<String, HashMap<String, String>>,
```

`active_text[message_id]` 指向当前 text part；`active_reasoning[message_id][reasoning_id]`
指向对应 reasoning part。`TextBlockEnd` 按 message ID 收尾 text，`ReasoningBlockEnd` 必须按
`reasoning_id` 精确收尾，不能退回“当前活跃 part”推断。

## 8.4 事件序列对比

> 参考（无直接开发任务）

改造前（当前）：
```
Messages(thinking, "Let me")  → translator: 无活跃 reasoning → 新建 reasoning
Messages(thinking, " think")  → translator: 已有活跃 reasoning → 追加
Messages(text, "Running")     → translator: 类型翻转 → finalize + 新建 text
Messages(text, " ls")         → translator: 已有活跃 text → 追加
ToolCall { ... }              → translator: finalize text
```

改造后：
```
TurnStart                          → translator: 创建 step-start part
  ReasoningBlockStart { id: "r0", metadata } → translator: 创建并记录 reasoning part
  ReasoningDelta { id: "r0", content: "Let me", metadata } → translator: 按 id 追加
  ReasoningDelta { id: "r0", content: " think", metadata } → translator: 按 id 追加
  ReasoningBlockEnd { id: "r0", metadata } → translator: 按 id 收尾并移除映射
  TextBlockStart { metadata }      → translator: 创建并记录 text part
  TextDelta { content: "Running", metadata } → translator: 追加
  TextDelta { content: " ls", metadata } → translator: 追加
  TextBlockEnd { metadata }        → translator: 收尾并移除 text 映射
  ToolCall { ... }                 → translator: 无需 finalize
TurnFinish { reason, usage }       → translator: 创建 step-finish part
```

## 8.5 各 Provider SSE 映射

> 开发任务：D1（openai_compat BlockTracker 嵌入）、D2（Usage 聚合）

**Anthropic**（天然三段式，直接映射）：

| Anthropic SSE | → StreamEvent |
|---|---|
| `message_start` | `TurnStart` |
| `content_block_start` (type=text) | `TextBlockStart` |
| `content_block_delta` (text_delta) | `TextDelta` |
| `content_block_start` (type=thinking) | `ReasoningBlockStart { id: block.index }` |
| `content_block_delta` (thinking_delta) | `ReasoningDelta { id: block.index }` |
| `content_block_stop` | `TextBlockEnd` 或 `ReasoningBlockEnd { id }`（按当前 block 类型） |
| `message_delta` (stop_reason) | `TurnFinish { reason, ... }` |
| `message_stop` | `Finish` |

**OpenAI**（无三段式，需 `BlockTracker` 状态追踪）：

| OpenAI SSE 字段 | → StreamEvent | 状态追踪 |
|---|---|---|
| stream 开始（首个 SSE） | `TurnStart` | — |
| `delta.content` 首次非空 | `TextBlockStart` → `TextDelta` | 进入 text block |
| `delta.content` 后续 | `TextDelta` | 维持 text block |
| `delta.reasoning_content` 首次非空 | `TextBlockEnd`(如有) → `ReasoningBlockStart { id }` → `ReasoningDelta { id }` | 退出 text → 进入 reasoning |
| `delta.reasoning_content` 后续 | `ReasoningDelta { id }` | 维持 reasoning block |
| `delta.content` 在 reasoning 后再次出现 | `ReasoningBlockEnd { id }` → `TextBlockStart` → `TextDelta` | 退出 reasoning → 进入 text |
| `finish_reason` 到达 | `TextBlockEnd` 或 `ReasoningBlockEnd`（按当前 block）→ `TurnFinish { reason, ... }` | 清空状态 |
| `finish_reason: "error"` | `ProviderError` | — |

## 8.6 BlockTracker 实现（OpenAI compat）

> 开发任务：C1（BlockTracker 新建）、C2（lib.rs 注册）

`BlockTracker` 不再产出 `LlmStreamEvent`，而是直接产出 `StreamEvent`：

```rust
// foundation/stream-event/src/block_tracker.rs

use crate::types::metadata::StreamMetadata;
use crate::StreamEvent;

#[derive(Debug, Clone, PartialEq)]
enum ActiveBlockKind {
    None,
    Text,
    Reasoning { id: String },
}

pub struct BlockTracker<S: Clone + Send + Sync + std::fmt::Debug + 'static> {
    active: ActiveBlockKind,
    reasoning_seq: usize,
    _phantom: std::marker::PhantomData<S>,
}

impl<S: Clone + Send + Sync + std::fmt::Debug + 'static> BlockTracker<S> {
    pub fn new() -> Self {
        Self {
            active: ActiveBlockKind::None,
            reasoning_seq: 0,
            _phantom: Default::default(),
        }
    }

    /// 处理一条 text delta，返回 0-3 个 StreamEvent
    /// （类型翻转时先 End 旧 block，再 Start 新 block，最后 TextDelta）。
    pub fn on_text_delta(
        &mut self,
        text: &str,
        metadata: &StreamMetadata,
    ) -> Vec<StreamEvent<S>> {
        let mut events = Vec::new();
        if self.active != ActiveBlockKind::Text {
            events.extend(self.close_current(metadata));
            self.active = ActiveBlockKind::Text;
            events.push(StreamEvent::TextBlockStart { metadata: metadata.clone() });
        }
        events.push(StreamEvent::TextDelta {
            content: text.to_string(),
            metadata: metadata.clone(),
        });
        events
    }

    /// 处理一条 reasoning delta，返回 0-3 个 StreamEvent。
    pub fn on_reasoning_delta(
        &mut self,
        text: &str,
        metadata: &StreamMetadata,
    ) -> Vec<StreamEvent<S>> {
        let mut events = Vec::new();
        let id = match &self.active {
            ActiveBlockKind::Reasoning { id } => id.clone(),
            _ => {
                events.extend(self.close_current(metadata));
                let id = format!("r{}", self.reasoning_seq);
                self.reasoning_seq += 1;
                self.active = ActiveBlockKind::Reasoning { id: id.clone() };
                events.push(StreamEvent::ReasoningBlockStart {
                    id: id.clone(),
                    metadata: metadata.clone(),
                });
                id
            }
        };
        events.push(StreamEvent::ReasoningDelta {
            id,
            content: text.to_string(),
            metadata: metadata.clone(),
        });
        events
    }

    /// 流结束时关闭当前 block，并追加 Finish。
    pub fn on_finish(&mut self, metadata: &StreamMetadata) -> Vec<StreamEvent<S>> {
        let mut events = self.close_current(metadata);
        events.push(StreamEvent::Finish);
        events
    }

    fn close_current(&mut self, metadata: &StreamMetadata) -> Vec<StreamEvent<S>> {
        let mut events = Vec::new();
        match std::mem::replace(&mut self.active, ActiveBlockKind::None) {
            ActiveBlockKind::Text => {
                events.push(StreamEvent::TextBlockEnd { metadata: metadata.clone() });
            }
            ActiveBlockKind::Reasoning { id } => {
                events.push(StreamEvent::ReasoningBlockEnd { id, metadata: metadata.clone() });
            }
            ActiveBlockKind::None => {}
        }
        events
    }
}
```

### BlockTracker 集成方式

当前生产链路：

```
llm_client.rs::send_chunk(sink, MessageChunk::thinking(msg), node_id)
  → sink.try_send_message(chunk, node_id)
  → StreamEventSink: StreamEvent::Messages { chunk, metadata }
  → stream_tx.try_send(event)
  → translate_and_emit → translate_stream_event
```

改造后 `send_chunk` 替换为 `BlockTracker`：

```rust
// llm_client.rs — 改造后
// sink 类型从 &dyn StreamSink 改为 &mut BlockTracker<S> + &Sender<StreamEvent<S>>
// 或：BlockTracker 自身持有 sender，方法返回 ()（内部直接 try_send）

fn send_stream_event(
    tx: &tokio::sync::mpsc::Sender<StreamEvent<S>>,
    event: StreamEvent<S>,
    first_chunk_at: &mut Option<std::time::Instant>,
) {
    if first_chunk_at.is_none() {
        *first_chunk_at = Some(std::time::Instant::now());
    }
    let _ = tx.try_send(event);
}
```

llm_client 调用处改为：

```rust
// 替代 send_chunk(sink, MessageChunk::thinking(reasoning_content), node_id, &mut first_chunk_at)
for ev in tracker.on_reasoning_delta(&reasoning_content, &metadata) {
    send_stream_event(tx, ev, &mut first_chunk_at);
}
```

BlockTracker 方法签名（返回 `Vec<StreamEvent>`）的设计理由：
- `on_text_delta` / `on_reasoning_delta` 在类型翻转时需产出 End + Start + Delta 三个事件
- 返回 `Vec` 让调用方逐条 `tx.try_send`，保持与现有 `send_chunk` 的一次性语义一致
- 替代方案（BlockTracker 持有 sender + 方法返回 `()`）也可行，但侵入性更大（需改 BlockTracker 泛型签名）

### 实际落地架构

设计文档描述的是终态（`llm_client → BlockTracker → StreamEvent`），实际因 `invoke_stream` trait
有 17 个实现、签名改动风险过大，落地为适配层：

```
设计:  llm_client → BlockTracker → StreamEvent → stream_tx → translator
实际:  llm_client → MessageChunk → BlockTrackerSink(impl StreamSink)
                                    → BlockTracker.on_text_delta / on_reasoning_delta
                                    → StreamEvent → stream_tx → translator
```

`BlockTrackerSink`（`agent/agent-core/src/agent/react/think_node.rs`）实现 `StreamSink` trait，
在 `try_send_message(chunk)` 内部按 `chunk.is_thinking()` 路由到 BlockTracker，
产出的 `StreamEvent` 直接 `stream_tx.try_send`。

功能等价于设计目标，但保留了 `StreamSink` 接口和 `MessageChunk` 类型作为中间层。

> 开发任务：E2-E17（translator 重写）、F2（translate_and_emit dispatch）

| 文件 | 改动 | 工作量 |
|---|---|---|
| `foundation/stream-event/src/types/stream_event.rs` | 追加显式 delta、block、turn 和错误事件；迁移后删除 `Messages` | 中 |
| `foundation/stream-event/src/block_tracker.rs` | 新建 `BlockTracker<S>` | 中 |
| `foundation/llm/src/client/openai_compat/llm_client.rs` | SSE parser 内嵌 `BlockTracker`，产出带边界的 `StreamEvent` | **大** |
| `foundation/llm/src/client/anthropic_compat/` | `content_block_*` 直接映射 `TextBlockStart/End` / `ReasoningBlockStart/End` | **小** |
| `agent/agent-core/src/agent/react/think_node.rs` | stream 结束时发 `TextBlockEnd`/`ReasoningBlockEnd`/`Finish`/`TurnFinish` | 中 |
| `apps/server/src/translator.rs` | match 覆盖 12 个相关 arm，删除 `translate_chunk` / `state.active`，改用 `active_text` + `active_reasoning` | 中 |

收尾接口定义，全部基于 §8.3.1 的确定性映射：

```rust
fn finalize_text_part(state: &AppState, session_id: &str, message_id: &str);
fn finalize_reasoning_part(
    state: &AppState,
    session_id: &str,
    message_id: &str,
    reasoning_id: &str,
);

fn finalize_all_reasoning_parts(
    state: &AppState,
    session_id: &str,
    message_id: &str,
);
```

### translator 简化效果

```rust
// translator.rs — 新版 translate_stream_event

fn translate_stream_event(event: &StreamEvent<ReActState>, ...) {
    match event {
        StreamEvent::TextBlockStart { metadata } => {
            let part_id = new_part_id();
            push_part(state, msg_id, session_id, "text", json!({
                "id": part_id, "type": "text", "text": "",
                "time": { "start": now_ms(), "created": now_ms() }, "metadata": metadata,
            }));
            state.active_text.write().insert(msg_id.to_string(), part_id);
        }

        StreamEvent::TextDelta { content, metadata } => {
            append_to_active_text(state, msg_id, content);
            emit_part_updated_with_metadata(state, session_id, msg_id, metadata);
        }

        StreamEvent::TextBlockEnd { .. } => {
            finalize_text_part(state, session_id, msg_id);
            state.active_text.write().remove(msg_id);
        }

        StreamEvent::ReasoningBlockStart { id, metadata } => {
            let part_id = new_part_id();
            push_reasoning_part(state, msg_id, session_id, part_id.clone(), id, metadata);
            state.active_reasoning.write()
                .entry(msg_id.to_string()).or_default()
                .insert(id.clone(), part_id);
        }

        StreamEvent::ReasoningDelta { id, content, metadata } => {
            append_to_reasoning(state, msg_id, id, content);
            emit_part_updated_with_metadata(state, session_id, msg_id, id, metadata);
        }

        StreamEvent::ReasoningBlockEnd { id, .. } => {
            finalize_reasoning_part(state, session_id, msg_id, id);
            if let Some(parts) = state.active_reasoning.write().get_mut(msg_id) {
                parts.remove(id);
            }
        }

        StreamEvent::ToolCall { call_id, name, arguments } => {
            // block 结束事件已先行到达，无需额外收尾
            create_or_update_tool_part(
                state, msg_id, session_id, call_id.as_deref(), name,
                ToolTransition::Create { input: arguments.clone() },
            );
        }

        StreamEvent::ToolError { call_id, error } => {
            fail_tool_call(state, call_id.as_deref(), error);
        }

        StreamEvent::ProviderError { message } => {
            // 进入错误处理路径（见 05-error-handling.md）
            return Err(message);
        }

        StreamEvent::TurnStart => {
            // 创建 step-start part
            let part_id = new_part_id();
            push_part(state, msg_id, session_id, "step-start", json!({
                "id": part_id, "type": "step-start",
                "time": { "start": now_ms() },
            }));
        }

        StreamEvent::TurnFinish { reason, usage } => {
            finalize_all_reasoning_parts(state, session_id, msg_id);
            let part_id = new_part_id();
            push_part(state, msg_id, session_id, "step-finish", json!({
                "id": part_id, "type": "step-finish",
                "reason": reason,
                "tokens": {
                    "prompt": usage.prompt_tokens,
                    "completion": usage.completion_tokens,
                    "total": usage.total_tokens,
                    "cached": usage.cached_tokens,
                },
                "time": { "start": now_ms(), "end": now_ms() },
            }));
        }

        StreamEvent::Finish => {
            // no-op（等价于 OpenCode case "finish": return）
        }

        // ... 其余已有变体不变
    }
}
```

与 OpenCode 的 `case "text-start": ... case "text-end": ...` **一一对应**，
不再需要 `translate_chunk` 的 4 步推断逻辑。

## 8.8 与 02 文档的关系

> 参考（无直接开发任务）

[02-part-lifecycle.md](02-part-lifecycle.md) 描述了当前 `translate_chunk` + part-type 搜索的问题和推导过程。
本文档（08）是直接落地的目标设计：用显式 block 事件替代推断，
删除 `Messages`、`MessageChunk`、`translate_chunk` 和独立 `Usage`。

## 8.9 事件不变量

> 参考（无直接开发任务）

- `TextDelta` 必须位于同一 `TextBlockStart` 与 `TextBlockEnd` 之间。
- `ReasoningDelta { id }` 必须位于同一 ID 的 `ReasoningBlockStart` 与 `ReasoningBlockEnd` 之间。
- `ToolCall` 前必须先结束当前 text/reasoning block。
- `TurnStart` / `TurnFinish` 包裹单次 LLM 调用；`TurnFinish` 携带该回合最终 usage。
- translator 必须按 reasoning ID 路由和收尾，不能根据最近事件类型推断。

## 8.10 Wire 协议变更（B2/B3）

> 开发任务：B2（`wire/convert.rs`）、B3（`wire/protocol.rs`）

### 当前 wire 协议

`ProtocolEvent` 枚举（`wire/protocol.rs`）定义了 checkpoint 持久化格式：

```rust
pub enum ProtocolEvent<S> {
    MessageChunk { kind: MessageChunkKind, content: String, metadata: StreamMetadata, .. },
    // ... tool, checkpoint, usage variants
}
```

`wire/convert.rs` 在 `StreamEvent ↔ ProtocolEvent` 之间双向转换：

```rust
// L70 — StreamEvent → ProtocolEvent
StreamEvent::Messages { chunk, metadata } => ProtocolEvent::MessageChunk {
    kind: chunk.kind.into(),
    content: chunk.content,
    metadata,
}
```

### 改造后 wire 协议

删除 `ProtocolEvent::MessageChunk`，替换为：

```rust
pub enum ProtocolEvent<S> {
    TextDelta { content: String, metadata: StreamMetadata, .. },
    ReasoningDelta { id: String, content: String, metadata: StreamMetadata, .. },
    TextBlockStart { metadata: StreamMetadata },
    TextBlockEnd { metadata: StreamMetadata },
    ReasoningBlockStart { id: String, metadata: StreamMetadata },
    ReasoningBlockEnd { id: String, metadata: StreamMetadata },
    TurnStart,
    TurnFinish { reason: String, usage: Usage },
    // ... tool, checkpoint variants (unchanged)
}
```

转换映射更新（`convert.rs`）：

| `StreamEvent` | → `ProtocolEvent` | 注意 |
|---|---|---|
| `TextDelta { content, metadata }` | `TextDelta { content, metadata }` | 1:1 |
| `ReasoningDelta { id, content, metadata }` | `ReasoningDelta { id, content, metadata }` | 1:1 |
| `TextBlockStart { metadata }` | `TextBlockStart { metadata }` | 1:1 |
| `TextBlockEnd { metadata }` | `TextBlockEnd { metadata }` | 1:1 |
| `ReasoningBlockStart { id, metadata }` | `ReasoningBlockStart { id, metadata }` | 1:1 |
| `ReasoningBlockEnd { id, metadata }` | `ReasoningBlockEnd { id, metadata }` | 1:1 |
| `TurnStart` | `TurnStart` | 1:1 |
| `TurnFinish { reason, usage }` | `TurnFinish { reason, usage }` | 1:1 |
| `ToolError / ProviderError / Finish` | 同名 `ProtocolEvent` 变体 | 1:1 |

**Checkpoint 兼容**：旧 checkpoint 中序列化的 `ProtocolEvent::MessageChunk` 需在反序列化时由适配器按 `kind` 转换为 `TextDelta` / `ReasoningDelta`。转换完成后删除适配器。

### 测试影响（`convert.rs` 测试）

以下测试需更新（行号基于当前代码）：

| 行号 | 当前 | 改造后 |
|---|---|---|
| L406, L422 | `MessageChunkKind::Message` round-trip | `TextDelta` round-trip |
| L530, L556 | `MessageChunkKind::Thinking` round-trip | `ReasoningDelta` round-trip |
| L854 | `StreamEvent::Messages` → `ProtocolEvent::MessageChunk` | `TextDelta` → `TextDelta` |
