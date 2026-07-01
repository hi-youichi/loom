//! Core LLM traits and types.
//!
//! This module defines the contract between Loom's agent runtime
//! and LLM provider implementations.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::message::Message;
use crate::error::LlmError;
use loom_graph_core::GraphError;

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

/// Response from an LLM completion: assistant message text and optional tool calls.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LlmResponse {
    /// Assistant message content (plain text).
    pub content: String,
    /// Optional model reasoning/thinking content, separate from the final assistant reply.
    pub reasoning_content: Option<String>,
    /// Tool calls from this turn; empty means no tools, observe → END.
    pub tool_calls: Vec<crate::tool::ToolCall>,
    /// Token usage for this call, when available (e.g. OpenAI returns this).
    pub usage: Option<LlmUsage>,
    /// Time when the first streamed chunk was emitted during streaming, used for
    /// prefill/decode duration computation in usage events.
    ///
    /// `None` when streaming was disabled (sink is `None`), the LLM does not stream
    /// by character, or no chunks were emitted (e.g. cached / prebuilt responses).
    pub first_chunk_at: Option<std::time::Instant>,
}

impl LlmResponse {
    /// Creates a simple text response without tool calls.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            reasoning_content: None,
            tool_calls: vec![],
            usage: None,
            first_chunk_at: None,
        }
    }

    /// Returns true if this response has no content and no tool calls.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty() && self.tool_calls.is_empty()
    }
}

// Re-export streaming types from stream-event for backward compatibility
pub use stream_event::message::{MessageChunk, MessageChunkKind, StreamSink};

/// Provider-level factory that can create [`LlmClient`] instances for different model names.
///
/// Holds connection configuration (base_url, api_key) and resolves tier abstractions
/// (Light / Standard / Strong) to concrete model IDs via [`ModelRegistry`](crate::registry::ModelRegistry).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Create a new [`LlmClient`] for the given model name.
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, GraphError>;

    /// Create a new [`LlmClient`] with optional HTTP headers.
    ///
    /// Default implementation falls back to [`Self::create_client`] (ignoring headers).
    /// Providers that support custom headers should override this.
    fn create_client_with_headers(
        &self,
        model: &str,
        headers: Option<LlmHeaders>,
    ) -> Result<Box<dyn LlmClient>, GraphError> {
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
/// The trait supports streaming via `invoke_stream()`. When `sink` is `Some`, implementations
/// should call `sink.try_send_message(chunk, node_id)` as tokens arrive from the LLM. The
/// method is **non-blocking** (a typical sink uses `try_send` internally) — implementations
/// must never `.await` on a channel send inside `invoke_stream`. The method still returns the
/// complete `LlmResponse` at the end, with `first_chunk_at` populated by the sink.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Invokes the model for one turn.
    ///
    /// Implementations should treat `messages` as the full prompt context for
    /// the current turn and return the fully assembled assistant response.
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, LlmError>;

    /// Streaming variant: invoke with optional sink for token streaming.
    ///
    /// When `sink` is `Some`, implementations should call `sink.try_send_message(chunk, node_id)`
    /// as tokens arrive from the LLM. The sink is **non-blocking** — implementations must never
    /// `.await` on a channel send here. The method returns the complete `LlmResponse` after all
    /// tokens are collected, with `first_chunk_at` populated (via the sink) when streaming was used.
    ///
    /// `node_id` is forwarded to every `try_send_message` call so the sink can tag outgoing
    /// events with the originating graph node.
    ///
    /// Default implementation calls `invoke()` and sends the full content as one chunk.
    async fn invoke_stream(
        &self,
        messages: &[Message],
        sink: Option<&dyn StreamSink>,
        node_id: &str,
    ) -> Result<LlmResponse, LlmError> {
        let response = self.invoke(messages).await?;

        // Default: send full content as single chunk if streaming is enabled
        if let Some(s) = sink {
            if let Some(ref reasoning_content) = response.reasoning_content {
                if !reasoning_content.is_empty() {
                    let _ = s.try_send_message(
                        MessageChunk::thinking(reasoning_content.clone()),
                        node_id,
                    );
                }
            }
            if !response.content.is_empty() {
                let _ = s.try_send_message(
                    MessageChunk::message(response.content.clone()),
                    node_id,
                );
            }
        }

        Ok(response)
    }

    /// List available models from the provider's /v1/models endpoint.
    ///
    /// Returns a list of models available from this provider. Not all providers
    /// support this endpoint; implementations should return an empty Vec or
    /// an appropriate error if unsupported.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        // Default: not supported, return empty list
        Ok(Vec::new())
    }
}

// ============================================================================
// Conversions from reqwest errors
// ============================================================================
//
// Note: `From<reqwest::Error> for GraphError` and `From<reqwest::Error> for LlmError`
// are defined in `error.rs`.

#[cfg(test)]
mod tests {
    use super::*;

    // LlmHeaders tests
    #[test]
    fn llm_headers_default_is_empty() {
        let headers = LlmHeaders::default();
        assert!(headers.thread_id.is_none());
        assert!(headers.trace_id.is_none());
        assert!(headers.custom_headers.is_empty());
    }

    #[test]
    fn llm_headers_builder_methods() {
        let headers = LlmHeaders::default()
            .with_thread_id("thread-123")
            .with_trace_id("trace-456")
            .add_header("X-Custom-Header", "custom-value")
            .add_header("X-Another-Header", "another-value");
        
        assert_eq!(headers.thread_id.as_deref(), Some("thread-123"));
        assert_eq!(headers.trace_id.as_deref(), Some("trace-456"));
        assert_eq!(headers.custom_headers.get("X-Custom-Header"), Some(&"custom-value".to_string()));
        assert_eq!(headers.custom_headers.get("X-Another-Header"), Some(&"another-value".to_string()));
        assert_eq!(headers.custom_headers.len(), 2);
    }

    #[test]
    fn llm_headers_from_env_when_not_set() {
        let headers = LlmHeaders::from_env();
        assert!(headers.thread_id.is_none());
        assert!(headers.trace_id.is_none());
        assert!(headers.custom_headers.is_empty());
    }

    // ToolChoiceMode tests
    #[test]
    fn tool_choice_mode_default_is_auto() {
        let mode = ToolChoiceMode::default();
        assert_eq!(mode, ToolChoiceMode::Auto);
    }

    #[test]
    fn tool_choice_mode_from_str_valid() {
        assert_eq!("auto".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::Auto);
        assert_eq!("none".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::None);
        assert_eq!("required".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::Required);
    }

    #[test]
    fn tool_choice_mode_from_str_case_insensitive() {
        assert_eq!("Auto".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::Auto);
        assert_eq!("AUTO".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::Auto);
        assert_eq!("NONE".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::None);
        assert_eq!("Required".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::Required);
        assert_eq!("REQUIRED".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::Required);
    }

    #[test]
    fn tool_choice_mode_from_str_invalid() {
        assert!("invalid".parse::<ToolChoiceMode>().is_err());
        assert!("".parse::<ToolChoiceMode>().is_err());
        assert!("random".parse::<ToolChoiceMode>().is_err());
        
        let err = "bogus".parse::<ToolChoiceMode>().unwrap_err();
        assert!(err.contains("unknown tool_choice"));
        assert!(err.contains("bogus"));
    }

    #[test]
    fn tool_choice_mode_serde_roundtrip() {
        let modes = vec![ToolChoiceMode::Auto, ToolChoiceMode::None, ToolChoiceMode::Required];
        
        for mode in modes {
            let serialized = serde_json::to_string(&mode).unwrap();
            let deserialized: ToolChoiceMode = serde_json::from_str(&serialized).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    // LlmUsage tests
    #[test]
    fn llm_usage_default() {
        let usage = LlmUsage::default();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
        assert!(usage.prompt_tokens_details.is_none());
        assert!(usage.completion_tokens_details.is_none());
    }

    #[test]
    fn llm_usage_accumulate() {
        let usage1 = LlmUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: Some(20),
                audio_tokens: None,
            }),
            completion_tokens_details: Some(CompletionTokensDetails {
                reasoning_tokens: Some(10),
                audio_tokens: None,
                accepted_prediction_tokens: None,
                rejected_prediction_tokens: None,
            }),
        };
        
        let usage2 = LlmUsage {
            prompt_tokens: 200,
            completion_tokens: 100,
            total_tokens: 300,
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: Some(30),
                audio_tokens: Some(5),
            }),
            completion_tokens_details: Some(CompletionTokensDetails {
                reasoning_tokens: Some(20),
                audio_tokens: Some(10),
                accepted_prediction_tokens: Some(5),
                rejected_prediction_tokens: Some(2),
            }),
        };
        
        let accumulated = usage1.accumulate(&usage2);
        assert_eq!(accumulated.prompt_tokens, 300);
        assert_eq!(accumulated.completion_tokens, 150);
        assert_eq!(accumulated.total_tokens, 450);
        assert!(accumulated.prompt_tokens_details.is_none());
        assert!(accumulated.completion_tokens_details.is_none());
    }

    #[test]
    fn llm_usage_accumulate_with_details() {
        let usage1 = LlmUsage {
            prompt_tokens: 50,
            completion_tokens: 25,
            total_tokens: 75,
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: Some(10),
                audio_tokens: Some(2),
            }),
            completion_tokens_details: Some(CompletionTokensDetails {
                reasoning_tokens: Some(5),
                audio_tokens: Some(3),
                accepted_prediction_tokens: Some(1),
                rejected_prediction_tokens: None,
            }),
        };
        
        let usage2 = LlmUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: Some(20),
                audio_tokens: None,
            }),
            completion_tokens_details: None,
        };
        
        let accumulated = usage1.accumulate(&usage2);
        assert_eq!(accumulated.prompt_tokens, 150);
        assert_eq!(accumulated.completion_tokens, 75);
        assert_eq!(accumulated.total_tokens, 225);
        assert!(accumulated.prompt_tokens_details.is_none());
        assert!(accumulated.completion_tokens_details.is_none());
    }

// LlmResponse tests
    #[test]
    fn llm_response_text_creates_response() {
        let response = LlmResponse::text("Hello, world!");
        assert_eq!(response.content, "Hello, world!");
        assert!(response.reasoning_content.is_none());
        assert!(response.tool_calls.is_empty());
        assert!(response.usage.is_none());
        assert!(response.first_chunk_at.is_none());
    }

    #[test]
    fn llm_response_is_empty_when_no_content_no_tools() {
        let response = LlmResponse::text("");
        assert!(response.is_empty());
    }

    #[test]
    fn llm_response_is_not_empty_with_content() {
        let response = LlmResponse::text("Some content");
        assert!(!response.is_empty());
    }

    #[test]
    fn llm_response_is_not_empty_with_tool_calls() {
        let response = LlmResponse {
            content: String::new(),
            reasoning_content: None,
            tool_calls: vec![crate::tool::ToolCall::new("test_tool", "{}")],
            usage: None,
            first_chunk_at: None,
        };
        assert!(!response.is_empty());
    }

    // MessageChunk tests
    #[test]
    fn message_chunk_message_factory() {
        let chunk = MessageChunk::message("test content");
        assert_eq!(chunk.content, "test content");
        assert_eq!(chunk.kind, MessageChunkKind::Message);
        assert!(!chunk.is_thinking());
    }

    #[test]
    fn message_chunk_thinking_factory() {
        let chunk = MessageChunk::thinking("thinking content");
        assert_eq!(chunk.content, "thinking content");
        assert_eq!(chunk.kind, MessageChunkKind::Thinking);
        assert!(chunk.is_thinking());
    }

    #[test]
    fn message_chunk_is_thinking() {
        let message_chunk = MessageChunk::message("content");
        assert!(!message_chunk.is_thinking());
        
        let thinking_chunk = MessageChunk::thinking("thinking");
        assert!(thinking_chunk.is_thinking());
    }

    #[test]
    fn message_chunk_default() {
        let chunk = MessageChunk::default();
        assert!(chunk.content.is_empty());
        assert_eq!(chunk.kind, MessageChunkKind::Message);
        assert!(!chunk.is_thinking());
    }

    #[test]
    fn message_chunk_kind_default() {
        let kind = MessageChunkKind::default();
        assert_eq!(kind, MessageChunkKind::Message);
    }

    // PromptTokensDetails & CompletionTokensDetails tests
    #[test]
    fn prompt_tokens_details_default() {
        let details = PromptTokensDetails::default();
        assert!(details.cached_tokens.is_none());
        assert!(details.audio_tokens.is_none());
    }

    #[test]
    fn completion_tokens_details_default() {
        let details = CompletionTokensDetails::default();
        assert!(details.reasoning_tokens.is_none());
        assert!(details.audio_tokens.is_none());
        assert!(details.accepted_prediction_tokens.is_none());
        assert!(details.rejected_prediction_tokens.is_none());
    }
}