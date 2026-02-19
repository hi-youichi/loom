//! OpenAI-compatible chat completion request DTOs.
//!
//! Used by the SSE adapter to parse incoming request bodies. Field names match
//! the [OpenAI Chat Completions API](https://platform.openai.com/docs/api-reference/chat).
//! Message `content` can be a string or an array of parts (multimodal); we accept both.

use serde::Deserialize;

/// Chat completion request body (OpenAI-compatible).
///
/// Used to parse POST body for `/v1/chat/completions`. Callers use
/// [`parse_chat_request`](crate::openai_sse::parse_chat_request) to extract
/// `user_message`, `system_prompt`, and optional `thread_id` for the ReAct runner.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChatCompletionRequest {
    /// List of messages (system, user, assistant). Last user message is used as input.
    pub messages: Vec<ChatMessage>,
    /// Model name (e.g. "gpt-4o-mini"). Echoed in response; actual model is server-configured.
    pub model: String,
    /// When true, response is streamed as SSE. Default true for this adapter.
    #[serde(default = "default_true")]
    pub stream: bool,
    /// Optional stream options (e.g. include_usage in the final chunk).
    #[serde(default)]
    pub stream_options: Option<StreamOptions>,
    /// Optional thread id for checkpointing multi-turn conversations (extension).
    #[serde(default)]
    pub thread_id: Option<String>,
    /// Optional working folder path for file tools (extension). When set, must exist and be a directory.
    #[serde(default)]
    pub working_folder: Option<String>,
    /// Optional approval policy: "none" | "destructive_only" | "always" (extension).
    #[serde(default)]
    pub approval_policy: Option<String>,
}

fn default_true() -> bool {
    true
}

/// A single message in the chat request.
///
/// Matches OpenAI message shape: role + content. Content can be a string or an
/// array of parts (e.g. `[{"type":"text","text":"..."}]`) for multimodal input.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChatMessage {
    /// Role: "system", "user", or "assistant".
    pub role: String,
    /// Message content: string or array of content parts. Use [`MessageContent::as_text`] to get text.
    pub content: Option<MessageContent>,
}

/// Message content: either a plain string or an array of parts (OpenAI multimodal).
///
/// Deserializes from `"hello"` or `[{"type":"text","text":"hello"},{"type":"image_url",...}]`
/// so that clients can send either format without "invalid type: sequence, expected a string".
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    String(String),
    Array(Vec<ContentPart>),
}

impl MessageContent {
    /// Returns the text of this content: the string variant as-is, or concatenation of
    /// all `text` fields from array parts (e.g. `type: "text"`). Other part types are skipped.
    pub fn as_text(&self) -> String {
        match self {
            MessageContent::String(s) => s.clone(),
            MessageContent::Array(parts) => parts
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        MessageContent::String(s)
    }
}

/// One part of a multimodal message content array (OpenAI format).
#[derive(Debug, Clone, Deserialize)]
pub struct ContentPart {
    /// Part type, e.g. "text", "image_url". Other fields (image_url, etc.) are ignored for extraction.
    #[serde(rename = "type")]
    pub part_type: Option<String>,
    /// Text content when type is "text".
    pub text: Option<String>,
}

/// Stream options for chat completion (OpenAI stream_options).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct StreamOptions {
    /// If true, include usage in the final stream chunk.
    #[serde(default)]
    pub include_usage: bool,
}
