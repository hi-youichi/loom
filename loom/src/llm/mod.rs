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
//! - [`ChatOpenAI`] and [`ChatOpenAICompat`] are concrete provider implementations.
//!
//! # Streaming
//!
//! [`LlmClient::invoke_stream`] and
//! [`LlmClient::invoke_stream_with_tool_delta`] let providers surface tokens and
//! incremental tool-call arguments while still returning a fully assembled
//! [`LlmResponse`] at the end of the turn.

mod mock;
mod model_cache;
mod model_registry;
mod retry;

use tokio::sync::mpsc;

/// Tool choice mode for chat completions: when tools are present, controls whether
/// the model may choose (auto), must not use (none), or must use (required).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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

/// HTTP headers configuration for LLM requests.
#[derive(Debug, Clone, Default)]
pub struct LlmHeaders {
    /// Thread identifier (X-Thread-Id header)
    pub thread_id: Option<String>,
    /// Trace identifier (X-Trace-Id header)
    pub trace_id: Option<String>,
    /// Custom additional headers
    pub custom_headers: std::collections::HashMap<String, String>,
}

impl LlmHeaders {
    /// Set the thread identifier for X-Thread-Id header
    pub fn with_thread_id(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }

    /// Set the trace identifier for X-Trace-Id header
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Add a custom header
    pub fn add_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom_headers.insert(key.into(), value.into());
        self
    }
}

/// Load LLM headers from environment variables.
///
/// Environment variables:
/// - `LLM_THREAD_ID`: Thread identifier  
/// - `LLM_TRACE_ID`: Trace identifier
pub fn get_headers_from_env() -> LlmHeaders {
    LlmHeaders {
        thread_id: std::env::var("LLM_THREAD_ID").ok(),
        trace_id: std::env::var("LLM_TRACE_ID").ok(),
        custom_headers: std::collections::HashMap::new(),
    }
}

pub(crate) mod thinking;
pub(crate) mod tool_call_accumulator;

mod openai;
mod openai_compat;
mod openai_provider;
mod openai_compat_provider;
mod fixed_provider;

pub use openai_compat::ChatOpenAICompat;
pub use openai_provider::OpenAIProvider;
pub use openai_compat_provider::OpenAICompatProvider;
pub use fixed_provider::FixedLlmProvider;

/// Deprecated alias for [`ChatOpenAICompat`].
#[deprecated(note = "renamed to ChatOpenAICompat")]
pub type ChatBigModel = ChatOpenAICompat;

pub use mock::MockLlm;
pub use model_cache::{fetch_provider_models, ModelCache, ProviderModels};
pub use model_registry::{create_llm_provider, create_llm_client, ModelEntry, ModelRegistry, ProviderConfig};
pub use openai::ChatOpenAI;
pub use retry::RetryLlmClient;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::message::Message;
use crate::model_spec::ModelTier;
use crate::state::ToolCall;
use crate::stream::MessageChunk;

/// Model information returned by provider's /v1/models endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Model identifier (e.g., "gpt-4", "claude-3-opus")
    pub id: String,
    /// Unix timestamp when the model was created
    pub created: Option<i64>,
    /// Owner/organization of the model
    pub owned_by: Option<String>,
}

/// Capability flags for a model.
#[derive(Debug, Clone, Default)]
pub struct ModelCapabilities {
    /// Supports chat completions (/v1/chat/completions)
    pub chat_completions: bool,
    /// Supports streaming responses
    pub streaming: bool,
    /// Supports function/tool calling
    pub tools: bool,
    /// Supports vision/image inputs
    pub vision: bool,
}

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

/// Breakdown of prompt tokens (OpenAI `prompt_tokens_details`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptTokensDetails {
    /// Cached tokens present in the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
    /// Audio tokens present in the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u32>,
}

/// Breakdown of completion tokens (OpenAI `completion_tokens_details`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompletionTokensDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_prediction_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_prediction_tokens: Option<u32>,
}

/// Token usage for one LLM call (prompt + completion).
///
/// Aligns with OpenAI Chat Completions [`CompletionUsage`](https://platform.openai.com/docs/api-reference/completions/object):
/// three top-level counts plus optional detail objects when the provider returns them.
///
/// **Interaction**: Optional part of `LlmResponse`; emitted as `StreamEvent::Usage`
/// when streaming so CLI can print usage when `--verbose`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LlmUsage {
    /// Tokens in the prompt (input).
    pub prompt_tokens: u32,
    /// Tokens in the completion (output).
    pub completion_tokens: u32,
    /// Total tokens (prompt + completion).
    pub total_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

impl LlmUsage {
    /// Sums headline token counts with `other`. Per-turn breakdown fields are cleared
    /// because OpenAI usage is per request and details are not additive across turns.
    pub fn accumulate(&self, other: &LlmUsage) -> LlmUsage {
        LlmUsage {
            prompt_tokens: self.prompt_tokens + other.prompt_tokens,
            completion_tokens: self.completion_tokens + other.completion_tokens,
            total_tokens: self.total_tokens + other.total_tokens,
            prompt_tokens_details: None,
            completion_tokens_details: None,
        }
    }
}

/// Response from an LLM completion: assistant message text and optional tool calls.
///
/// **Interaction**: Returned by `LlmClient::invoke()`; ThinkNode writes
/// `content` into a new assistant message and `tool_calls` into `ReActState::tool_calls`.
#[derive(Debug, Clone, PartialEq)]
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

/// Provider-level factory that can create [`LlmClient`] instances for different model names.
///
/// Holds connection configuration (base_url, api_key) and resolves tier abstractions
/// (Light / Standard / Strong) to concrete model IDs via [`ModelRegistry`].
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Create a new [`LlmClient`] for the given model name.
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError>;

    /// Default model ID for this provider (used when `ModelConfig` has no explicit model).
    fn default_model(&self) -> &str;

    /// Provider name (e.g. `"openai"`, `"bigmodel"`).
    fn provider_name(&self) -> &str;

    /// Resolve a [`ModelTier`] to a concrete model ID.
    ///
    /// For [`ModelTier::None`], implementations should return [`Self::default_model`].
    async fn resolve_tier(&self, tier: ModelTier) -> Result<String, AgentError>;
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

    /// List available models from the provider's /v1/models endpoint.
    ///
    /// Returns a list of models available from this provider. Not all providers
    /// support this endpoint; implementations should return an empty Vec or
    /// an appropriate error if unsupported.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, AgentError> {
        // Default: not supported, return empty list
        Ok(Vec::new())
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

    #[test]
    fn test_llm_headers_builder() {
        let headers = LlmHeaders::default()
            .with_thread_id("test-thread")
            .with_trace_id("test-trace");

        assert_eq!(headers.thread_id, Some("test-thread".to_string()));
        assert_eq!(headers.trace_id, Some("test-trace".to_string()));
    }

    #[test]
    fn test_custom_headers() {
        let headers = LlmHeaders::default()
            .add_header("X-Custom", "value")
            .add_header("X-Another", "another-value");

        assert_eq!(
            headers.custom_headers.get("X-Custom"),
            Some(&"value".to_string())
        );
        assert_eq!(
            headers.custom_headers.get("X-Another"),
            Some(&"another-value".to_string())
        );
    }

    #[test]
    fn test_llm_headers_default() {
        let headers = LlmHeaders::default();
        assert!(headers.thread_id.is_none());
        assert!(headers.trace_id.is_none());
        assert!(headers.custom_headers.is_empty());
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
