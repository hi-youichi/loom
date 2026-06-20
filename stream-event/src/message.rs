//! Streaming message types: chunks and sink trait.
//!
//! These types define how LLM output is chunked and delivered during streaming.
//! They live here (not in `loom-llm`) to avoid a circular dependency:
//! `loom-llm → loom-graph → stream-event`.

use serde::{Deserialize, Serialize};

/// Distinguishes reasoning/thinking output from final assistant message for streaming.
///
/// When an LLM emits separate thinking content (e.g. extended thinking, reasoning tokens),
/// chunks with `Thinking` are streamed as ACP `agent_thought_chunk`; `Message` as `agent_message_chunk`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageChunkKind {
    /// Final assistant reply; maps to ACP `agent_message_chunk`.
    #[default]
    Message,
    /// Agent reasoning/thinking; maps to ACP `agent_thought_chunk`.
    Thinking,
}

/// One chunk of streamed message content.
///
/// Use [`MessageChunkKind`] to separate thinking from final reply when the LLM provides both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageChunk {
    pub content: String,
    /// When `Thinking`, ACP bridge emits `agent_thought_chunk`; otherwise `agent_message_chunk`.
    #[allow(clippy::struct_field_names)]
    pub kind: MessageChunkKind,
}

impl MessageChunk {
    /// Chunk of final assistant message (ACP `agent_message_chunk`).
    pub fn message(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            kind: MessageChunkKind::Message,
        }
    }

    /// Chunk of agent reasoning/thinking (ACP `agent_thought_chunk`).
    pub fn thinking(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            kind: MessageChunkKind::Thinking,
        }
    }

    /// Returns `true` if this is a thinking/reasoning chunk.
    pub fn is_thinking(&self) -> bool {
        self.kind == MessageChunkKind::Thinking
    }
}

impl Default for MessageChunk {
    fn default() -> Self {
        Self {
            content: String::new(),
            kind: MessageChunkKind::Message,
        }
    }
}

/// Sink that receives streamed `MessageChunk`s as they arrive from the LLM.
///
/// Implementations are typically lightweight (e.g. [`StreamEventSink`](crate::StreamEventSink)
/// which packs chunks into `StreamEvent::Messages` and forwards to the main `stream_tx`).
/// The sink must be `Send + Sync` and the `try_send_message` method **must not block** —
/// it is called from inside the LLM's SSE parsing loop, and any backpressure here would
/// stall token parsing. If the channel is full or closed, implementations should drop
/// the chunk silently rather than awaiting.
pub trait StreamSink: Send + Sync {
    /// Sends one streamed message chunk through to the underlying stream pipeline.
    ///
    /// `node_id` identifies the LLM caller (typically the graph node, e.g. `"think"`).
    /// Implementations pack it into the outgoing `StreamMetadata` so consumers can
    /// route the chunk back to the originating node.
    ///
    /// Returns `None` after a successful send, or `Some(Instant)` **on the first chunk**
    /// so the caller can record `first_chunk_at` for prefill/decode timing. Implementations
    /// must return `Some(now)` exactly once per session (the very first chunk) and `None`
    /// afterwards.
    fn try_send_message(&self, chunk: MessageChunk, node_id: &str) -> Option<std::time::Instant>;
}
