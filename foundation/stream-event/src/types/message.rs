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
}
