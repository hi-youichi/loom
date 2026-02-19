//! OpenAI-compatible Chat Completions SSE adapter.
//!
//! Converts [`StreamEvent<ReActState>`](crate::stream::StreamEvent) into SSE lines
//! in the format of [OpenAI streaming](https://platform.openai.com/docs/api-reference/chat-streaming).
//! No HTTP dependency: callers feed events and consume SSE lines (or bytes).
//!
//! # Types
//!
//! - **[`ChatCompletionRequest`]**: Request body DTO (messages, model, stream, stream_options, thread_id).
//! - **[`ChatCompletionChunk`]**: Response chunk DTO (id, object, created, model, choices, usage).
//! - **[`StreamToSse`]**: Stateful adapter that turns `StreamEvent<ReActState>` into SSE lines.
//! - **[`parse_chat_request`]**: Parses request into `user_message`, `system_prompt`, `RunnableConfig`.
//!
//! # Example
//!
//! ```ignore
//! let mut adapter = StreamToSse::new(ChunkMeta { id: "chatcmpl-1".into(), model: "gpt-4o".into(), .. }, true);
//! run_react_graph_stream(..., Some(|ev| adapter.feed(ev)));
//! adapter.finish();
//! for line in adapter.take_lines() { write!(body, "{}", line); }
//! ```

mod chunk;
mod parse;
mod request;

pub use chunk::{
    ChatCompletionChunk, ChunkChoice, ChunkUsage, Delta, DeltaToolCall, DeltaToolCallFunction,
};
pub use parse::{parse_chat_request, ParseError, ParsedChatRequest};
pub use request::{ChatCompletionRequest, ChatMessage, MessageContent, StreamOptions};

use crate::state::ReActState;
use crate::stream::StreamEvent;
use chunk::ChatCompletionChunk as Chunk;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

/// Metadata for all chunks in one stream (same id, created, model).
#[derive(Debug, Clone)]
pub struct ChunkMeta {
    /// Completion id (e.g. "chatcmpl-xxx").
    pub id: String,
    /// Model name to echo in chunks.
    pub model: String,
    /// Unix timestamp (seconds). If None, uses current time at first chunk.
    pub created: Option<u64>,
}

impl ChunkMeta {
    /// Resolves created timestamp: uses self.created or current time.
    pub fn created_secs(&mut self) -> u64 {
        if let Some(c) = self.created {
            c
        } else {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            self.created = Some(secs);
            secs
        }
    }
}

/// Converts `StreamEvent<ReActState>` into OpenAI SSE lines.
///
/// Feed events via [`feed`](StreamToSse::feed); then call [`finish`](StreamToSse::finish) and
/// [`take_lines`](StreamToSse::take_lines) to get `data: <JSON>\n\n` strings. When constructed
/// with [`new_with_sink`](StreamToSse::new_with_sink), each line is also sent to the channel
/// as it is produced (for HTTP streaming). Holds optional pending usage for the final chunk.
pub struct StreamToSse {
    meta: ChunkMeta,
    include_usage: bool,
    usage: Option<ChunkUsage>,
    lines: Vec<String>,
    sent_initial: bool,
    /// When set, each produced line is also sent here (e.g. for SSE response body).
    sink: Option<mpsc::Sender<String>>,
}

impl StreamToSse {
    /// Builds a new adapter with the given chunk metadata and options.
    pub fn new(meta: ChunkMeta, include_usage: bool) -> Self {
        Self {
            meta,
            include_usage,
            usage: None,
            lines: Vec::new(),
            sent_initial: false,
            sink: None,
        }
    }

    /// Builds a new adapter that also sends each SSE line to `sink` as it is produced.
    /// Use for HTTP streaming: the response body can read from the receiver.
    pub fn new_with_sink(meta: ChunkMeta, include_usage: bool, sink: mpsc::Sender<String>) -> Self {
        Self {
            meta,
            include_usage,
            usage: None,
            lines: Vec::new(),
            sent_initial: false,
            sink: Some(sink),
        }
    }

    fn push_line(&mut self, line: String) {
        if let Some(ref tx) = self.sink {
            let _ = tx.try_send(line.clone());
        }
        self.lines.push(line);
    }

    /// Feeds one stream event and may push one or more SSE lines into the internal buffer.
    ///
    /// Call [`take_lines`](StreamToSse::take_lines) after the stream ends to retrieve them.
    pub fn feed(&mut self, event: StreamEvent<ReActState>) {
        let created = self.meta.created_secs();
        let id = self.meta.id.clone();
        let model = self.meta.model.clone();

        match event {
            StreamEvent::TaskStart { node_id } if node_id == "think" && !self.sent_initial => {
                self.sent_initial = true;
                let chunk = Chunk {
                    id: id.clone(),
                    object: Chunk::OBJECT,
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta {
                            role: Some("assistant".to_string()),
                            content: Some(String::new()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                };
                self.push_line(write_sse_line(&chunk));
            }
            StreamEvent::Messages { chunk, .. } => {
                let chunk = Chunk {
                    id: id.clone(),
                    object: Chunk::OBJECT,
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta {
                            role: None,
                            content: Some(chunk.content),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                };
                self.push_line(write_sse_line(&chunk));
            }
            StreamEvent::Updates { state, .. } if !state.tool_calls.is_empty() => {
                let tool_calls: Vec<DeltaToolCall> = state
                    .tool_calls
                    .iter()
                    .enumerate()
                    .map(|(i, tc)| DeltaToolCall {
                        index: i as u32,
                        id: tc.id.clone(),
                        r#type: Some("function".to_string()),
                        function: Some(DeltaToolCallFunction {
                            name: Some(tc.name.clone()),
                            arguments: Some(tc.arguments.clone()),
                        }),
                    })
                    .collect();
                let chunk = Chunk {
                    id: id.clone(),
                    object: Chunk::OBJECT,
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta {
                            role: None,
                            content: None,
                            tool_calls: Some(tool_calls),
                        },
                        finish_reason: Some("tool_calls".to_string()),
                    }],
                    usage: None,
                };
                self.push_line(write_sse_line(&chunk));
            }
            StreamEvent::Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens,
            } => {
                self.usage = Some(ChunkUsage {
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                });
            }
            StreamEvent::Values(_) => {
                // Do not emit here: we emit the final chunk only in finish() after stream ends.
            }
            StreamEvent::Custom(value) => {
                let line = format!(
                    "data: {}\n\n",
                    serde_json::to_string(&serde_json::json!({
                        "type": "loom_custom",
                        "payload": value
                    }))
                    .expect("custom value serialization")
                );
                self.push_line(line);
            }
            _ => {}
        }
    }

    /// Emits the final chunk (delta: {}, finish_reason: "stop", optional usage).
    /// Call this once after the stream has ended (e.g. after the last event was fed).
    pub fn finish(&mut self) {
        let created = self.meta.created_secs();
        let chunk = Chunk {
            id: self.meta.id.clone(),
            object: Chunk::OBJECT,
            created,
            model: self.meta.model.clone(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta::default(),
                finish_reason: Some("stop".to_string()),
            }],
            usage: if self.include_usage {
                self.usage.clone()
            } else {
                None
            },
        };
        self.push_line(write_sse_line(&chunk));
    }

    /// Returns and clears the collected SSE lines.
    pub fn take_lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }
}

/// Serializes a [`ChatCompletionChunk`] to a single SSE line: `data: <JSON>\n\n`.
///
/// Used by [`StreamToSse`] and by HTTP handlers that write the response body.
pub fn write_sse_line(chunk: &ChatCompletionChunk) -> String {
    let json = serde_json::to_string(chunk).expect("chunk serialization is infallible");
    format!("data: {json}\n\n")
}
