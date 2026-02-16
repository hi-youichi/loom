//! Mock LLM for tests and examples.
//!
//! Returns fixed assistant message and optional fixed ToolCall (e.g. get_time);
//! configurable "no tool_calls" to test END path. Optional stateful mode for multi-round.
//!
//! # Streaming Support
//!
//! `MockLlm` implements `invoke_stream()` with configurable streaming behavior:
//! - Default: sends content as a single chunk (efficient for most tests)
//! - Character-by-character: splits content into individual character chunks (for stream testing)

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::AgentError;
use crate::llm::{LlmClient, LlmResponse};
use crate::message::Message;
use crate::state::ToolCall;
use crate::stream::MessageChunk;

/// Mock LLM: fixed assistant text and optional tool_calls.
///
/// Configurable to return one fixed ToolCall (e.g. get_time) or no tool_calls,
/// so the graph can run one round (think → act → observe → END) or test END
/// after think. Used by ThinkNode in tests and ReAct linear example.
/// Optional stateful mode: first call returns tool_calls, second returns no tool_calls (multi-round).
///
/// # Streaming
///
/// By default, `invoke_stream()` sends the content as a single chunk. Enable
/// `stream_by_char` to send each character as a separate chunk (useful for testing).
///
/// **Interaction**: Implements `LlmClient`; used by ThinkNode.
pub struct MockLlm {
    /// Assistant message content to return (or first call when stateful).
    content: String,
    /// Tool calls to return (or first call when stateful).
    tool_calls: Vec<ToolCall>,
    /// When Some, first invoke() returns (content, tool_calls), later returns (second_content, []).
    call_count: Option<AtomicUsize>,
    /// Second response content (stateful mode).
    second_content: Option<String>,
    /// When true, invoke_stream sends each character as a separate chunk.
    stream_by_char: AtomicBool,
}

impl MockLlm {
    /// Creates a mock that returns one assistant message and one tool call (get_time).
    ///
    /// Fixed single assistant message and single ToolCall (e.g. get_time) for tests.
    pub fn with_get_time_call() -> Self {
        Self {
            content: "I'll check the time.".to_string(),
            tool_calls: vec![ToolCall {
                name: "get_time".to_string(),
                arguments: "{}".to_string(),
                id: Some("call-1".to_string()),
            }],
            call_count: None,
            second_content: None,
            stream_by_char: AtomicBool::new(false),
        }
    }

    /// Creates a mock that returns assistant text and no tool_calls (END path).
    pub fn with_no_tool_calls(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            tool_calls: vec![],
            call_count: None,
            second_content: None,
            stream_by_char: AtomicBool::new(false),
        }
    }

    /// Creates a mock with custom content and tool_calls.
    pub fn new(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            content: content.into(),
            tool_calls,
            call_count: None,
            second_content: None,
            stream_by_char: AtomicBool::new(false),
        }
    }

    /// Creates a stateful mock: first invoke() returns get_time tool_call, second returns no tool_calls.
    /// Used for multi-round ReAct tests (phase 5).
    pub fn first_tools_then_end() -> Self {
        Self {
            content: "I'll check the time.".to_string(),
            tool_calls: vec![ToolCall {
                name: "get_time".to_string(),
                arguments: "{}".to_string(),
                id: Some("call-1".to_string()),
            }],
            call_count: Some(AtomicUsize::new(0)),
            second_content: Some("The time is as above.".to_string()),
            stream_by_char: AtomicBool::new(false),
        }
    }

    /// Set content (builder).
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    /// Set tool_calls (builder).
    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    /// Enable character-by-character streaming for `invoke_stream()`.
    ///
    /// When enabled, each character of the content is sent as a separate `MessageChunk`.
    /// This is useful for testing streaming behavior.
    pub fn with_stream_by_char(self) -> Self {
        self.stream_by_char.store(true, Ordering::SeqCst);
        self
    }
}

#[async_trait]
impl LlmClient for MockLlm {
    async fn invoke(&self, _messages: &[Message]) -> Result<LlmResponse, AgentError> {
        let (content, tool_calls) = match &self.call_count {
            Some(c) => {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    (self.content.clone(), self.tool_calls.clone())
                } else {
                    (
                        self.second_content
                            .as_deref()
                            .unwrap_or(&self.content)
                            .to_string(),
                        vec![],
                    )
                }
            }
            None => (self.content.clone(), self.tool_calls.clone()),
        };
        Ok(LlmResponse {
            content,
            tool_calls,
            usage: None,
        })
    }

    /// Streaming variant: sends content chunks through the channel.
    ///
    /// Behavior depends on `stream_by_char`:
    /// - false (default): sends entire content as one chunk
    /// - true: sends each character as a separate chunk (for testing)
    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError> {
        // Get the response content (handles stateful mode)
        let response = self.invoke(messages).await?;

        // Send chunks if streaming is enabled
        if let Some(tx) = chunk_tx {
            if !response.content.is_empty() {
                if self.stream_by_char.load(Ordering::SeqCst) {
                    // Character-by-character streaming
                    for c in response.content.chars() {
                        let _ = tx
                            .send(MessageChunk {
                                content: c.to_string(),
                            })
                            .await;
                    }
                } else {
                    // Single chunk (default)
                    let _ = tx
                        .send(MessageChunk {
                            content: response.content.clone(),
                        })
                        .await;
                }
            }
        }

        Ok(response)
    }
}
