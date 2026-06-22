//! Stream chunk DTOs for SSE parsing.
//!
//! These types model the JSON shape of individual `data:` lines in the
//! Server-Sent Events stream returned by OpenAI-compatible providers.

/// Function fragment inside a streamed tool-call delta.
#[derive(serde::Deserialize, Default)]
pub(super) struct StreamDeltaFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

/// A single tool-call delta in a stream chunk.
#[derive(serde::Deserialize, Default)]
pub(super) struct StreamToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    pub function: Option<StreamDeltaFunction>,
}

/// Content delta for one choice in a stream chunk.
#[derive(serde::Deserialize, Default)]
pub(super) struct StreamDelta {
    pub content: Option<String>,
    #[serde(default, alias = "reasoning", alias = "reason_content")]
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
}

/// One choice inside a stream chunk.
#[derive(serde::Deserialize)]
pub(super) struct StreamChoice {
    pub delta: StreamDelta,
    /// OpenAI-compatible; optional so we don't fail if the API omits it.
    #[serde(default)]
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

/// Top-level SSE chunk from `/chat/completions` with `stream: true`.
#[derive(serde::Deserialize)]
pub(super) struct StreamChunk {
    pub choices: Option<Vec<StreamChoice>>,
    pub usage: Option<super::request::ResponseUsage>,
}
