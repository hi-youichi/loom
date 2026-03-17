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
                ..
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{MessageChunk, StreamMetadata};
    use crate::state::{ReActState, ToolCall};

    fn meta_with_created(created: u64) -> ChunkMeta {
        ChunkMeta {
            id: "chatcmpl-test".into(),
            model: "gpt-4o".into(),
            created: Some(created),
        }
    }

    fn meta_without_created() -> ChunkMeta {
        ChunkMeta {
            id: "chatcmpl-test".into(),
            model: "gpt-4o".into(),
            created: None,
        }
    }

    #[test]
    fn chunk_meta_created_secs_uses_provided_value() {
        let mut meta = meta_with_created(12345);
        assert_eq!(meta.created_secs(), 12345);
        assert_eq!(meta.created, Some(12345));
    }

    #[test]
    fn chunk_meta_created_secs_uses_current_time_when_none() {
        let mut meta = meta_without_created();
        let secs = meta.created_secs();
        assert!(secs > 0);
        assert_eq!(meta.created, Some(secs));
        // Second call returns cached value
        assert_eq!(meta.created_secs(), secs);
    }

    #[test]
    fn write_sse_line_formats_correctly() {
        let chunk = ChatCompletionChunk {
            id: "id".into(),
            object: "chat.completion.chunk",
            created: 1000,
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
        };
        let line = write_sse_line(&chunk);
        assert!(line.starts_with("data: "));
        assert!(line.ends_with("\n\n"));
        assert!(line.contains("\"id\":\"id\""));
        assert!(line.contains("\"created\":1000"));
    }

    #[test]
    fn stream_to_sse_task_start_think_emits_initial_chunk() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::TaskStart {
            node_id: "think".into(),
        });
        let lines = adapter.take_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("assistant"));
        assert!(lines[0].contains("data: "));
    }

    #[test]
    fn stream_to_sse_task_start_non_think_ignored() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::TaskStart {
            node_id: "act".into(),
        });
        assert!(adapter.take_lines().is_empty());
    }

    #[test]
    fn stream_to_sse_messages_emits_content_chunk() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::Messages {
            chunk: MessageChunk::message("hello world"),
            metadata: StreamMetadata {
                loom_node: "think".into(),
            },
        });
        let lines = adapter.take_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("hello world"));
    }

    #[test]
    fn stream_to_sse_updates_with_tool_calls_emits_tool_calls_chunk() {
        let mut state = ReActState::default();
        state.tool_calls = vec![ToolCall {
            id: Some("call_1".into()),
            name: "get_weather".into(),
            arguments: r#"{"city":"NYC"}"#.into(),
        }];
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::Updates {
            node_id: "act".into(),
            state,
        });
        let lines = adapter.take_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("tool_calls"));
        assert!(lines[0].contains("get_weather"));
    }

    #[test]
    fn stream_to_sse_updates_empty_tool_calls_ignored() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::Updates {
            node_id: "act".into(),
            state: ReActState::default(),
        });
        assert!(adapter.take_lines().is_empty());
    }

    #[test]
    fn stream_to_sse_usage_stores_for_finish() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), true);
        adapter.feed(StreamEvent::Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            prefill_duration: None,
            decode_duration: None,
        });
        adapter.finish();
        let lines = adapter.take_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("usage"));
        assert!(lines[0].contains("30"));
    }

    #[test]
    fn stream_to_sse_finish_without_usage_omits_usage() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::Usage {
            prompt_tokens: 5,
            completion_tokens: 10,
            total_tokens: 15,
            prefill_duration: None,
            decode_duration: None,
        });
        adapter.finish();
        let lines = adapter.take_lines();
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].contains("\"usage\""));
    }

    #[test]
    fn stream_to_sse_values_does_not_emit() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::Values(ReActState::default()));
        assert!(adapter.take_lines().is_empty());
    }

    #[test]
    fn stream_to_sse_custom_emits_loom_custom_line() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::Custom(serde_json::json!({"key": "value"})));
        let lines = adapter.take_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("loom_custom"));
        assert!(lines[0].contains("\"key\":\"value\""));
    }

    #[test]
    fn stream_to_sse_other_events_ignored() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::TaskEnd {
            node_id: "think".into(),
            result: Ok(()),
        });
        adapter.feed(StreamEvent::TaskStart {
            node_id: "other".into(),
        });
        assert!(adapter.take_lines().is_empty());
    }

    #[test]
    fn stream_to_sse_take_lines_clears_buffer() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), false);
        adapter.feed(StreamEvent::Messages {
            chunk: MessageChunk::message("x"),
            metadata: StreamMetadata {
                loom_node: "think".into(),
            },
        });
        let lines1 = adapter.take_lines();
        assert_eq!(lines1.len(), 1);
        let lines2 = adapter.take_lines();
        assert!(lines2.is_empty());
    }

    #[test]
    fn stream_to_sse_new_with_sink_sends_to_channel() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut adapter = StreamToSse::new_with_sink(meta_with_created(1000), false, tx);
        adapter.feed(StreamEvent::Messages {
            chunk: MessageChunk::message("sink test"),
            metadata: StreamMetadata {
                loom_node: "think".into(),
            },
        });
        let lines = adapter.take_lines();
        assert_eq!(lines.len(), 1);
        // Sink should have received the same line
        let received = rx.try_recv().unwrap();
        assert!(received.contains("sink test"));
    }

    #[test]
    fn stream_to_sse_full_flow() {
        let mut adapter = StreamToSse::new(meta_with_created(1000), true);
        adapter.feed(StreamEvent::TaskStart {
            node_id: "think".into(),
        });
        adapter.feed(StreamEvent::Messages {
            chunk: MessageChunk::message("Hi"),
            metadata: StreamMetadata {
                loom_node: "think".into(),
            },
        });
        adapter.feed(StreamEvent::Messages {
            chunk: MessageChunk::message(" there"),
            metadata: StreamMetadata {
                loom_node: "think".into(),
            },
        });
        adapter.feed(StreamEvent::Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
            prefill_duration: None,
            decode_duration: None,
        });
        adapter.finish();
        let lines = adapter.take_lines();
        assert_eq!(lines.len(), 4); // initial + 2 messages + finish
        assert!(lines[0].contains("assistant"));
        assert!(lines[1].contains("Hi"));
        assert!(lines[2].contains(" there"));
        assert!(lines[3].contains("stop"));
        assert!(lines[3].contains("usage"));
    }
}
