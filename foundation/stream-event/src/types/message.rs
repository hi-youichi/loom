use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageChunkKind {
    #[default]
    Message,
    Thinking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageChunk {
    pub content: String,
    pub kind: MessageChunkKind,
}

/// A provider-native increment of a function/tool invocation.
///
/// Unlike a completed tool call, these are emitted at the exact point at
/// which the provider declares the call.  Consumers can therefore create a
/// stable placeholder without waiting for the whole response to finish.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolCallChunk {
    Started {
        call_id: String,
        name: String,
    },
    Delta {
        call_id: String,
        arguments_delta: String,
    },
    Ended {
        call_id: String,
        arguments: String,
    },
}

impl MessageChunk {
    pub fn message(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            kind: MessageChunkKind::Message,
        }
    }

    pub fn thinking(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            kind: MessageChunkKind::Thinking,
        }
    }

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

pub trait StreamSink: Send + Sync {
    fn try_send_message(&self, chunk: MessageChunk, node_id: &str) -> Option<std::time::Instant>;

    /// Forward a tool-call increment. Implementations that only render text
    /// can ignore it; the default preserves backwards compatibility.
    fn try_send_tool_call(
        &self,
        _chunk: ToolCallChunk,
        _node_id: &str,
    ) -> Option<std::time::Instant> {
        None
    }
}
