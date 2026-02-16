//! LLM client abstraction for ReAct Think node.
//!
//! ThinkNode depends on a callable that returns assistant text and optional
//! tool_calls; this module defines the trait and a mock implementation.
//!
//! # Streaming Support
//!
//! The `LlmClient` trait supports streaming via `invoke_stream()`, which accepts
//! an optional `Sender<MessageChunk>` for emitting tokens as they arrive.
//! Implementations that support streaming (like `ChatOpenAI`) will send chunks
//! through the channel; others (like `MockLlm`) can use the default implementation
//! that calls `invoke()` and optionally sends the full content as one chunk.

mod mock;

use tokio::sync::mpsc;

/// Tool choice mode for chat completions: when tools are present, controls whether
/// the model may choose (auto), must not use (none), or must use (required).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToolChoiceMode {
    /// Model can pick between message or tool calls. Default when tools are present.
    #[default]
    Auto,
    /// Model will not call any tool.
    None,
    /// Model must call one or more tools.
    Required,
}

impl std::str::FromStr for ToolChoiceMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "none" => Ok(Self::None),
            "required" => Ok(Self::Required),
            _ => Err(format!(
                "unknown tool_choice: {} (use auto, none, or required)",
                s
            )),
        }
    }
}

mod openai;

pub use mock::MockLlm;
pub use openai::ChatOpenAI;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::message::Message;
use crate::state::ToolCall;
use crate::stream::MessageChunk;

/// Token usage for one LLM call (prompt + completion).
///
/// **Interaction**: Optional part of `LlmResponse`; emitted as `StreamEvent::Usage`
/// when streaming so CLI can print usage when `--verbose`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LlmUsage {
    /// Tokens in the prompt (input).
    pub prompt_tokens: u32,
    /// Tokens in the completion (output).
    pub completion_tokens: u32,
    /// Total tokens (prompt + completion).
    pub total_tokens: u32,
}

/// Response from an LLM completion: assistant message text and optional tool calls.
///
/// **Interaction**: Returned by `LlmClient::invoke()`; ThinkNode writes
/// `content` into a new assistant message and `tool_calls` into `ReActState::tool_calls`.
pub struct LlmResponse {
    /// Assistant message content (plain text).
    pub content: String,
    /// Tool calls from this turn; empty means no tools, observe → END.
    pub tool_calls: Vec<ToolCall>,
    /// Token usage for this call, when available (e.g. OpenAI returns this).
    pub usage: Option<LlmUsage>,
}

/// LLM client: given messages, returns assistant text and optional tool_calls.
///
/// ThinkNode calls this to produce the next assistant message and any tool
/// invocations. Implementations: `MockLlm` (fixed response), `ChatOpenAI` (real API, feature `openai`).
///
/// # Streaming
///
/// The trait supports streaming via `invoke_stream()`. When `chunk_tx` is `Some`,
/// implementations should send `MessageChunk` tokens through the channel as they
/// arrive from the LLM. The method still returns the complete `LlmResponse` at the end.
///
/// Default implementation calls `invoke()` and optionally sends the full content
/// as a single chunk.
///
/// **Interaction**: Used by ThinkNode.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Invoke one turn: read messages, return assistant content and optional tool_calls.
    /// Aligns with LangChain's `invoke` / `ainvoke` (single-call API).
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;

    /// Streaming variant: invoke with optional chunk sender for token streaming.
    ///
    /// When `chunk_tx` is `Some`, implementations should send `MessageChunk` tokens
    /// through the channel as they arrive. The method returns the complete `LlmResponse`
    /// after all tokens are collected.
    ///
    /// Default implementation calls `invoke()` and sends the full content as one chunk.
    ///
    /// # Arguments
    ///
    /// * `messages` - Input messages (system, user, assistant history)
    /// * `chunk_tx` - Optional sender for streaming message chunks
    ///
    /// # Returns
    ///
    /// Complete `LlmResponse` with full content and any tool_calls.
    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError> {
        let response = self.invoke(messages).await?;

        // Default: send full content as single chunk if streaming is enabled
        if let Some(tx) = chunk_tx {
            if !response.content.is_empty() {
                let _ = tx
                    .send(MessageChunk {
                        content: response.content.clone(),
                    })
                    .await;
            }
        }

        Ok(response)
    }
}
