//! LLM client abstraction for ReAct think steps.
//!
//! The types in this module define the contract between Loom's ReAct runtime and
//! model providers:
//!
//! - [`LlmClient`] is the provider trait used by [`crate::agent::react::ThinkNode`].
//! - [`LlmResponse`] carries assistant text, optional reasoning content, tool
//!   calls, and optional usage.
//! - [`ToolChoiceMode`] configures whether a provider may emit tool calls when
//!   tools are available.
//! - [`ChatOpenAI`] and [`ChatBigModel`] are concrete provider implementations.
//!
//! # Streaming
//!
//! [`LlmClient::invoke_stream`] and
//! [`LlmClient::invoke_stream_with_tool_delta`] let providers surface tokens and
//! incremental tool-call arguments while still returning a fully assembled
//! [`LlmResponse`] at the end of the turn.

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

mod bigmodel;
mod openai;

pub use bigmodel::ChatBigModel;
pub use mock::MockLlm;
pub use openai::ChatOpenAI;

pub mod context_persistence;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::message::Message;
use crate::state::ToolCall;
use crate::stream::MessageChunk;

/// Delta for one tool call from LLM streaming (for tool_call_chunk events).
#[derive(Clone, Debug)]
pub struct ToolCallDelta {
    /// Stable tool call id when the provider emits one.
    pub call_id: Option<String>,
    /// Tool/function name when the provider emits it.
    pub name: Option<String>,
    /// Incremental argument fragment for this tool call.
    pub arguments_delta: String,
}

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
    /// Optional model reasoning/thinking content, separate from the final assistant reply.
    pub reasoning_content: Option<String>,
    /// Tool calls from this turn; empty means no tools, observe → END.
    pub tool_calls: Vec<ToolCall>,
    /// Token usage for this call, when available (e.g. OpenAI returns this).
    pub usage: Option<LlmUsage>,
}

/// LLM client: given messages, returns assistant text and optional tool_calls.
///
/// [`crate::agent::react::ThinkNode`] calls this trait to produce the next
/// assistant message and any tool invocations. Implementations may wrap remote
/// APIs, local models, or test doubles such as [`MockLlm`].
///
/// # Streaming
///
/// The trait supports streaming via `invoke_stream()`. When `chunk_tx` is `Some`,
/// implementations should send `MessageChunk` tokens through the channel as they
/// arrive from the LLM. The method still returns the complete `LlmResponse` at the end.
///
/// Default implementation calls `invoke()` and optionally sends the full content
/// as a single chunk.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Invokes the model for one turn.
    ///
    /// Implementations should treat `messages` as the full prompt context for
    /// the current turn and return the fully assembled assistant response.
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;

    /// Streaming variant: invoke with optional chunk sender for token streaming.
    ///
    /// When `chunk_tx` is `Some`, implementations should send `MessageChunk` tokens
    /// through the channel as they arrive. The method returns the complete `LlmResponse`
    /// after all tokens are collected.
    ///
    /// Default implementation calls `invoke()` and sends the full content as one chunk.
    ///
    /// `messages` is the same full prompt context passed to [`Self::invoke`].
    /// `chunk_tx` is an opportunistic side channel for incremental output.
    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError> {
        let response = self.invoke(messages).await?;

        // Default: send full content as single chunk if streaming is enabled
        if let Some(tx) = chunk_tx {
            if let Some(ref reasoning_content) = response.reasoning_content {
                if !reasoning_content.is_empty() {
                    let _ = tx
                        .send(MessageChunk::thinking(reasoning_content.clone()))
                        .await;
                }
            }
            if !response.content.is_empty() {
                let _ = tx
                    .send(MessageChunk::message(response.content.clone()))
                    .await;
            }
        }

        Ok(response)
    }

    /// Streaming variant with tool call delta support.
    ///
    /// Like `invoke_stream`, but additionally sends `ToolCallDelta` through
    /// `tool_delta_tx` as the LLM produces tool call arguments incrementally.
    ///
    /// The default implementation delegates to [`Self::invoke_stream`] and emits
    /// no tool deltas.
    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
        _tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
    ) -> Result<LlmResponse, AgentError> {
        self.invoke_stream(messages, chunk_tx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubLlm {
        content: String,
    }

    #[async_trait]
    impl LlmClient for StubLlm {
        async fn invoke(&self, _messages: &[Message]) -> Result<LlmResponse, AgentError> {
            Ok(LlmResponse {
                content: self.content.clone(),
                reasoning_content: None,
                tool_calls: vec![],
                usage: None,
            })
        }
    }

    #[test]
    fn tool_choice_mode_from_str_parses_known_values() {
        assert_eq!(
            "auto".parse::<ToolChoiceMode>().unwrap(),
            ToolChoiceMode::Auto
        );
        assert_eq!(
            "none".parse::<ToolChoiceMode>().unwrap(),
            ToolChoiceMode::None
        );
        assert_eq!(
            "required".parse::<ToolChoiceMode>().unwrap(),
            ToolChoiceMode::Required
        );
    }

    #[test]
    fn tool_choice_mode_from_str_rejects_unknown_value() {
        let err = "unexpected".parse::<ToolChoiceMode>().unwrap_err();
        assert!(err.contains("unknown tool_choice"));
    }

    #[tokio::test]
    async fn default_invoke_stream_sends_single_chunk_when_enabled() {
        let llm = StubLlm {
            content: "hello".to_string(),
        };
        let (tx, mut rx) = mpsc::channel(2);
        let resp = llm.invoke_stream(&[], Some(tx)).await.unwrap();
        assert_eq!(resp.content, "hello");
        let chunk = rx.recv().await.expect("one chunk");
        assert_eq!(chunk.content, "hello");
    }

    #[tokio::test]
    async fn default_invoke_stream_skips_chunk_for_empty_content() {
        let llm = StubLlm {
            content: String::new(),
        };
        let (tx, mut rx) = mpsc::channel(2);
        let resp = llm.invoke_stream(&[], Some(tx)).await.unwrap();
        assert!(resp.content.is_empty());
        assert!(rx.try_recv().is_err());
    }
}
