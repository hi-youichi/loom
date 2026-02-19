//! OpenAI-compatible chat completion chunk (streaming response) DTOs.
//!
//! Each SSE line is `data: <JSON>\n\n` where JSON is a [`ChatCompletionChunk`].
//! Matches [OpenAI streaming](https://platform.openai.com/docs/api-reference/chat-streaming).

use serde::Serialize;

/// A single streamed chunk of a chat completion (object: "chat.completion.chunk").
///
/// Serialized as the JSON value in SSE `data:` lines. Consumed by OpenAI-compatible
/// clients. Built by [`StreamToSse`](crate::openai_sse::StreamToSse).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ChatCompletionChunk {
    /// Unique id for this completion; same for all chunks in the stream.
    pub id: String,
    /// Always "chat.completion.chunk".
    pub object: &'static str,
    /// Unix timestamp (seconds) when the completion was created.
    pub created: u64,
    /// Model name (echoed from request or server config).
    pub model: String,
    /// List of choices (typically one element; index 0).
    pub choices: Vec<ChunkChoice>,
    /// Usage statistics; present only in the final chunk when include_usage was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChunkUsage>,
}

/// One choice in a streamed chunk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ChunkChoice {
    /// Index of the choice (0 when n=1).
    pub index: u32,
    /// Delta for this chunk (role, content, or empty for finish).
    pub delta: Delta,
    /// Null until the final chunk; then "stop" or "tool_calls" etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Delta content for a streamed chunk.
///
/// First chunk may have role + empty content; content chunks have content only;
/// tool_calls chunk has tool_calls array and optionally finish_reason "tool_calls";
/// final chunk has empty delta.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct Delta {
    /// Role (e.g. "assistant"); only in the first chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Content delta (partial text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls delta; when present, choice typically has finish_reason "tool_calls".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<DeltaToolCall>>,
}

/// One tool call in a streamed delta (OpenAI streaming tool_calls format).
///
/// OpenAI expects `function: { name?, arguments? }` and clients may require
/// `type: "function"` ("Expected 'function' type" otherwise).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DeltaToolCall {
    /// Index of the tool call (0, 1, ...).
    pub index: u32,
    /// Tool call id (from LLM or generated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Must be "function" for function tool calls; some clients validate this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Function details; OpenAI SDK validates this object must be present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<DeltaToolCallFunction>,
}

/// Nested function payload for a streamed tool call (OpenAI delta.tool_calls[].function).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DeltaToolCallFunction {
    /// Tool/function name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Arguments JSON string (may be partial in streaming; we send full when we have it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// Token usage in the final chunk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ChunkUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl ChatCompletionChunk {
    /// Object type string for all chunks.
    pub const OBJECT: &'static str = "chat.completion.chunk";
}
