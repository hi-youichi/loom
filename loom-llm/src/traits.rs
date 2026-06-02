//! Core LLM traits and types.
//!
//! This module defines the contract between Loom's agent runtime
//! and LLM provider implementations.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::message::Message;
use crate::error::AgentError;

// ============================================================================
// Headers
// ============================================================================

/// HTTP headers for LLM requests.
#[derive(Debug, Clone, Default)]
pub struct LlmHeaders {
    /// Thread identifier (X-Thread-Id header)
    pub thread_id: Option<String>,
    /// Trace identifier (X-Trace-Id header)
    pub trace_id: Option<String>,
    /// Custom additional headers
    pub custom_headers: HashMap<String, String>,
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

    /// Load headers from environment variables (LLM_THREAD_ID, LLM_TRACE_ID)
    pub fn from_env() -> Self {
        Self {
            thread_id: std::env::var("LLM_THREAD_ID").ok(),
            trace_id: std::env::var("LLM_TRACE_ID").ok(),
            custom_headers: HashMap::new(),
        }
    }
}

// ============================================================================
// Configuration Types
// ============================================================================

/// Tool choice mode for chat completions: when tools are present,
/// controls whether the model may choose (auto), must not use (none),
/// or must use (required).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
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

// ============================================================================
// Usage and Response Types
// ============================================================================

/// Breakdown of prompt tokens (OpenAI `prompt_tokens_details`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptTokensDetails {
    /// Cached tokens present in the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
    /// Audio tokens present in the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u32>,
}

/// Breakdown of completion tokens (OpenAI `completion_tokens_details`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

// ============================================================================
// Response and Delta Types
// ============================================================================

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

/// Response from an LLM completion: assistant message text and optional tool calls.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    /// Assistant message content (plain text).
    pub content: String,
    /// Optional model reasoning/thinking content, separate from the final assistant reply.
    pub reasoning_content: Option<String>,
    /// Tool calls from this turn; empty means no tools, observe → END.
    pub tool_calls: Vec<crate::tool::ToolCall>,
    /// Token usage for this call, when available (e.g. OpenAI returns this).
    pub usage: Option<LlmUsage>,
}

impl LlmResponse {
    /// Creates a simple text response without tool calls.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            reasoning_content: None,
            tool_calls: vec![],
            usage: None,
        }
    }

    /// Returns true if this response has no content and no tool calls.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty() && self.tool_calls.is_empty()
    }
}

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
#[derive(Debug, Clone)]
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

// ============================================================================
// Core Traits
// ============================================================================

/// Provider-level factory that can create [`LlmClient`] instances for different model names.
///
/// Holds connection configuration (base_url, api_key) and resolves tier abstractions
/// (Light / Standard / Strong) to concrete model IDs via [`ModelRegistry`](crate::registry::ModelRegistry).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Create a new [`LlmClient`] for the given model name.
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError>;

    /// Create a new [`LlmClient`] with optional HTTP headers.
    ///
    /// Default implementation falls back to [`Self::create_client`] (ignoring headers).
    /// Providers that support custom headers should override this.
    fn create_client_with_headers(
        &self,
        model: &str,
        headers: Option<LlmHeaders>,
    ) -> Result<Box<dyn LlmClient>, AgentError> {
        let _ = headers;
        self.create_client(model)
    }

    /// Default model ID for this provider (used when `ModelConfig` has no explicit model).
    fn default_model(&self) -> &str;

    /// Provider name (e.g. `"openai"`, `"bigmodel"`).
    fn provider_name(&self) -> &str;
}

/// LLM client: given messages, returns assistant text and optional tool_calls.
///
/// [`LlmClient`] is called to produce assistant messages and tool invocations.
/// Implementations may wrap remote APIs, local models, or test doubles such as [`MockLlm`](crate::client::MockLlm).
///
/// # Streaming
///
/// The trait supports streaming via `invoke_stream()`. When `chunk_tx` is `Some`,
/// implementations should send `MessageChunk` tokens through the channel as they
/// arrive from the LLM. The method still returns the complete `LlmResponse` at the end.
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
        tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
    ) -> Result<LlmResponse, AgentError> {
        let _ = tool_delta_tx;
        self.invoke_stream(messages, chunk_tx).await
    }
}

// ============================================================================
// Provider Configuration
// ============================================================================

/// Provider configuration for creating LLM clients.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Provider name (e.g., "openai", "bigmodel", "deepseek")
    pub name: String,
    /// Base URL for the API (e.g., "https://api.openai.com/v1")
    pub base_url: String,
    /// API key for authentication
    pub api_key: String,
    /// Optional organization ID (OpenAI specific)
    pub organization: Option<String>,
    /// Default model for this provider
    pub default_model: String,
}

impl ProviderConfig {
    /// Creates a new provider configuration.
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            organization: None,
            default_model: default_model.into(),
        }
    }

    /// Sets the organization ID (OpenAI specific).
    pub fn with_organization(mut self, org: impl Into<String>) -> Self {
        self.organization = Some(org.into());
        self
    }
}

/// Model entry in the registry.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    /// Model identifier (e.g., "gpt-4", "gpt-3.5-turbo")
    pub model: String,
    /// Provider name this model belongs to
    pub provider: String,
    /// Optional display name
    pub display_name: Option<String>,
    /// Whether this model supports function calling
    pub supports_function_calling: bool,
    /// Whether this model supports vision
    pub supports_vision: bool,
    /// Context window size in tokens (if known)
    pub context_window: Option<u32>,
}

impl ModelEntry {
    /// Creates a new model entry.
    pub fn new(model: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            provider: provider.into(),
            display_name: None,
            supports_function_calling: false,
            supports_vision: false,
            context_window: None,
        }
    }

    /// Sets the display name.
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Enables function calling support.
    pub fn with_function_calling(mut self) -> Self {
        self.supports_function_calling = true;
        self
    }

    /// Enables vision support.
    pub fn with_vision(mut self) -> Self {
        self.supports_vision = true;
        self
    }

    /// Sets the context window size.
    pub fn with_context_window(mut self, size: u32) -> Self {
        self.context_window = Some(size);
        self
    }
}

// ============================================================================
// Conversions from reqwest errors
// ============================================================================

impl From<reqwest::Error> for AgentError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            AgentError::ExecutionFailed("LLM request timeout".into())
        } else if e.is_connect() {
            AgentError::ExecutionFailed(format!("LLM connection error: {}", e))
        } else {
            AgentError::ExecutionFailed(format!("LLM request failed: {}", e))
        }
    }
}