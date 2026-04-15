/// Distinguishes reasoning/thinking output from final assistant message for streaming.
///
/// When an LLM emits separate thinking content (e.g. extended thinking, reasoning tokens),
/// chunks with `Thinking` are streamed as ACP `agent_thought_chunk`; `Message` as `agent_message_chunk`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
#[derive(Clone, Debug)]
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
}

impl Default for MessageChunk {
    fn default() -> Self {
        Self {
            content: String::new(),
            kind: MessageChunkKind::Message,
        }
    }
}
