//! Stream response accumulation for OpenAI SSE chat completions.
//!
//! [`StreamAccumulator`] consumes `async_openai` stream chunks and
//! emits [`MessageChunk`] via a [`StreamSink`](crate::llm::StreamSink), while
//! assembling the final [`LlmResponse`](crate::llm::LlmResponse) content.
//!
//! Tool call deltas are accumulated internally into a [`ToolCallAccumulator`] but
//! **no longer pushed to a separate channel** — that channel only drained and had
//! no consumer. Final tool calls are exposed via [`StreamResult::tool_calls`] in
//! `finish()`.

use async_openai::types::chat::{
    ChatCompletionMessageToolCallChunk, CreateChatCompletionStreamResponse,
};

use crate::support::thinking::{
    collect_thinking_tags, strip_thinking_tags, ThinkingSegment, ThinkingTagParser,
};
use crate::support::tool_call_accumulator::{fallback_call_id, ToolCallAccumulator};
use crate::traits::MessageChunk;
use crate::traits::{LlmUsage, StreamSink, ToolCallChunk};

/// Accumulates streaming SSE chunks into a complete response.
pub(super) struct StreamAccumulator {
    full_content: String,
    tool_calls: ToolCallAccumulator,
    usage: Option<LlmUsage>,
    sent_any_content: bool,
    thinking_parser: Option<ThinkingTagParser>,
    parse_thinking_tags: bool,
    live_tool_calls: std::collections::HashMap<u32, LiveToolCall>,
}

#[derive(Default)]
struct LiveToolCall {
    call_id: String,
    name: String,
    arguments: String,
    started: bool,
}

pub(super) struct StreamResult {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub tool_calls: Vec<crate::tool::ToolCall>,
    pub usage: Option<LlmUsage>,
}

impl StreamAccumulator {
    /// Create a new accumulator.
    ///
    /// When `parse_thinking` is true, content deltas are routed through
    /// a [`ThinkingTagParser`] to split reasoning from message text.
    pub fn new(parse_thinking: bool) -> Self {
        Self {
            full_content: String::new(),
            tool_calls: ToolCallAccumulator::new(),
            usage: None,
            sent_any_content: false,
            thinking_parser: parse_thinking.then(ThinkingTagParser::new),
            parse_thinking_tags: parse_thinking,
            live_tool_calls: std::collections::HashMap::new(),
        }
    }

    /// Process one SSE stream response chunk.
    ///
    /// Forwards incremental content/thinking to `sink` (non-blocking) and
    /// accumulates tool calls internally. Returns `Some(Instant)` exactly once,
    /// on the very first chunk — useful for tracking first-token latency.
    pub fn process_chunk(
        &mut self,
        response: CreateChatCompletionStreamResponse,
        sink: &dyn StreamSink,
        node_id: &str,
    ) -> Option<std::time::Instant> {
        if let Some(ref u) = response.usage {
            self.usage = Some(super::completion_usage_to_llm(u));
        }

        let mut first_chunk = None;
        for choice in response.choices {
            let delta = &choice.delta;

            if let Some(ref content) = delta.content {
                if !content.is_empty() {
                    if first_chunk.is_none() {
                        first_chunk = self.process_content_delta(content, sink, node_id);
                    } else {
                        self.process_content_delta(content, sink, node_id);
                    }
                }
            }

            if let Some(ref tool_calls) = delta.tool_calls {
                self.process_tool_calls_delta(tool_calls, sink, node_id);
            }
        }
        first_chunk
    }

    fn send_thinking_segment(
        sink: &dyn StreamSink,
        seg: ThinkingSegment,
        node_id: &str,
    ) -> Option<std::time::Instant> {
        match seg {
            ThinkingSegment::Message(s) => sink.try_send_message(MessageChunk::message(s), node_id),
            ThinkingSegment::Thinking(s) => {
                sink.try_send_message(MessageChunk::thinking(s), node_id)
            }
        }
    }

    fn process_content_delta(
        &mut self,
        content: &str,
        sink: &dyn StreamSink,
        node_id: &str,
    ) -> Option<std::time::Instant> {
        self.full_content.push_str(content);
        self.sent_any_content = true;

        if let Some(ref mut parser) = self.thinking_parser {
            let mut first = None;
            for seg in parser.feed(content) {
                let r = Self::send_thinking_segment(sink, seg, node_id);
                if first.is_none() && r.is_some() {
                    first = r;
                }
            }
            first
        } else {
            sink.try_send_message(MessageChunk::message(content.to_owned()), node_id)
        }
    }

    fn process_tool_calls_delta(
        &mut self,
        tool_calls: &[ChatCompletionMessageToolCallChunk],
        sink: &dyn StreamSink,
        node_id: &str,
    ) {
        use crate::support::tool_call_accumulator::RawToolCallDelta;
        for tc in tool_calls {
            let name = tc.function.as_ref().and_then(|f| f.name.clone());
            let arguments = tc.function.as_ref().and_then(|f| f.arguments.clone());
            let live = self.live_tool_calls.entry(tc.index).or_insert_with(|| LiveToolCall {
                call_id: tc.id.clone().filter(|id| !id.is_empty()).unwrap_or_else(|| fallback_call_id(tc.index)),
                ..Default::default()
            });
            // The first visible id is immutable for the lifetime of the
            // call; see ToolCallAccumulator's late-id fallback rule.
            if let Some(ref name) = name {
                live.name.push_str(name);
            }
            if let Some(ref arguments) = arguments {
                live.arguments.push_str(arguments);
            }
            if !live.started && !live.name.is_empty() {
                live.started = true;
                let _ = sink.try_send_tool_call(
                    ToolCallChunk::Started { call_id: live.call_id.clone(), name: live.name.clone() },
                    node_id,
                );
                if !live.arguments.is_empty() {
                    let _ = sink.try_send_tool_call(
                        ToolCallChunk::Delta { call_id: live.call_id.clone(), arguments_delta: live.arguments.clone() },
                        node_id,
                    );
                }
            } else if live.started {
                if let Some(arguments) = arguments.clone() {
                    let _ = sink.try_send_tool_call(
                        ToolCallChunk::Delta { call_id: live.call_id.clone(), arguments_delta: arguments },
                        node_id,
                    );
                }
            }
            self.tool_calls.push(RawToolCallDelta {
                index: tc.index,
                id: tc.id.clone(),
                name,
                arguments,
            });

            tracing::trace!(
                index = %tc.index,
                id = ?tc.id,
                name = ?tc.function.as_ref().and_then(|f| f.name.as_deref()),
                arguments = ?tc.function.as_ref().and_then(|f| f.arguments.as_deref()),
                "tool_calls chunk"
            );
        }
    }

    /// Flush remaining thinking buffer and handle edge cases.
    ///
    /// Must be called after the stream ends, before `finish()`.
    pub fn flush(&mut self, sink: &dyn StreamSink, node_id: &str) -> Option<std::time::Instant> {
        if let Some(parser) = self.thinking_parser.take() {
            if let Some(seg) = parser.flush() {
                return Self::send_thinking_segment(sink, seg, node_id);
            }
        }
        None
    }

    /// Close every live tool input before the completed tool calls are handed
    /// to the agent. This preserves the provider's original event ordering.
    pub fn finish_tool_inputs(&mut self, sink: &dyn StreamSink, node_id: &str) {
        for call in self.live_tool_calls.values().filter(|call| call.started) {
            let _ = sink.try_send_tool_call(
                ToolCallChunk::Ended { call_id: call.call_id.clone(), arguments: call.arguments.clone() },
                node_id,
            );
        }
    }

    /// Send full content as one chunk if no incremental content was sent
    /// (some proxies only include content in the final payload).
    pub fn emit_full_if_needed(
        &self,
        sink: &dyn StreamSink,
        node_id: &str,
    ) -> Option<std::time::Instant> {
        if !self.sent_any_content && !self.full_content.is_empty() {
            sink.try_send_message(MessageChunk::message(self.full_content.clone()), node_id)
        } else {
            None
        }
    }

    /// Consume and produce final content, tool_calls, and usage.
    pub fn finish(self) -> StreamResult {
        let content = if self.parse_thinking_tags {
            strip_thinking_tags(&self.full_content)
        } else {
            self.full_content.clone()
        };
        let reasoning_content = collect_thinking_tags(&self.full_content);
        StreamResult {
            content,
            reasoning_content,
            tool_calls: self.tool_calls.finish(),
            usage: self.usage,
        }
    }
}

/// Test sink: counts calls and returns first-chunk timing.
#[cfg(test)]
pub(super) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use crate::traits::{MessageChunk, StreamSink};

    pub(super) struct CountingSink {
        pub count: AtomicUsize,
        pub first: Mutex<Option<std::time::Instant>>,
        pub chunks: Mutex<Vec<MessageChunk>>,
        pub node_ids: Mutex<Vec<String>>,
    }

    impl CountingSink {
        pub(super) fn new() -> Self {
            Self {
                count: AtomicUsize::new(0),
                first: Mutex::new(None),
                chunks: Mutex::new(Vec::new()),
                node_ids: Mutex::new(Vec::new()),
            }
        }
    }

    impl StreamSink for CountingSink {
        fn try_send_message(
            &self,
            chunk: MessageChunk,
            node_id: &str,
        ) -> Option<std::time::Instant> {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.chunks.lock().unwrap().push(chunk);
            self.node_ids.lock().unwrap().push(node_id.to_string());
            let mut guard = self.first.lock().unwrap();
            if guard.is_none() {
                *guard = Some(std::time::Instant::now());
                *guard
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(deprecated)]

    use super::test_support::CountingSink;
    use super::*;
    use crate::support::thinking::ThinkingSegment;
    use crate::traits::MessageChunkKind;
    use async_openai::types::chat::{
        ChatChoiceStream, ChatCompletionMessageToolCallChunk, ChatCompletionStreamResponseDelta,
        CreateChatCompletionStreamResponse, FunctionCallStream,
    };
    use std::sync::atomic::Ordering;

    fn empty_stream_response() -> CreateChatCompletionStreamResponse {
        CreateChatCompletionStreamResponse {
            id: "id".into(),
            choices: vec![],
            created: 0,
            model: "m".into(),
            system_fingerprint: None,
            object: "chat.completion.chunk".into(),
            usage: None,
            service_tier: None,
        }
    }

    fn delta_empty() -> ChatCompletionStreamResponseDelta {
        ChatCompletionStreamResponseDelta {
            content: None,
            function_call: None,
            refusal: None,
            role: None,
            tool_calls: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_thinking_segment_emits_message_chunk() {
        let sink = CountingSink::new();
        StreamAccumulator::send_thinking_segment(
            &sink,
            ThinkingSegment::Message("hi".into()),
            "think",
        );
        let chunks = sink.chunks.lock().unwrap();
        assert_eq!(chunks[0].content, "hi");
        assert_eq!(chunks[0].kind, MessageChunkKind::Message);
        assert_eq!(sink.node_ids.lock().unwrap()[0], "think");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_thinking_segment_emits_thinking_chunk() {
        let sink = CountingSink::new();
        StreamAccumulator::send_thinking_segment(
            &sink,
            ThinkingSegment::Thinking("r".into()),
            "think",
        );
        let chunks = sink.chunks.lock().unwrap();
        assert_eq!(chunks[0].content, "r");
        assert_eq!(chunks[0].kind, MessageChunkKind::Thinking);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn process_content_delta_plain_accumulates_and_sends_one_chunk() {
        let mut acc = StreamAccumulator::new(false);
        let sink = CountingSink::new();
        acc.process_content_delta("ab", &sink, "think");
        assert_eq!(acc.full_content, "ab");
        assert!(acc.sent_any_content);
        let chunks = sink.chunks.lock().unwrap();
        assert_eq!(chunks[0].content, "ab");
        assert_eq!(chunks[0].kind, MessageChunkKind::Message);
        assert_eq!(sink.count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn process_content_delta_with_thinking_parser_splits_kinds() {
        let mut acc = StreamAccumulator::new(true);
        let sink = CountingSink::new();
        let tag_s = crate::support::thinking::THINKING_START;
        let tag_e = crate::support::thinking::THINKING_END;
        acc.process_content_delta(&format!("a {}x{} b", tag_s, tag_e), &sink, "think");
        assert!(acc.sent_any_content);
        assert!(!acc.full_content.is_empty());
        let mut saw_message = false;
        let mut saw_thinking = false;
        for c in sink.chunks.lock().unwrap().iter() {
            match c.kind {
                MessageChunkKind::Message => saw_message = true,
                MessageChunkKind::Thinking => saw_thinking = true,
            }
        }
        assert!(saw_message);
        assert!(saw_thinking);
    }

    #[test]
    fn process_tool_calls_delta_accumulates_without_tool_channel() {
        let mut acc = StreamAccumulator::new(false);
        let chunks = [ChatCompletionMessageToolCallChunk {
            index: 0,
            id: Some("id1".into()),
            function: Some(FunctionCallStream {
                name: Some("n".into()),
                arguments: Some(r#"{"a":1}"#.into()),
            }),
            r#type: None,
        }];
        acc.process_tool_calls_delta(&chunks);
        let r = acc.finish();
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.tool_calls[0].name, "n");
    }

    #[test]
    fn process_tool_calls_delta_accumulates_arguments() {
        let mut acc = StreamAccumulator::new(false);
        let chunks = [ChatCompletionMessageToolCallChunk {
            index: 0,
            id: Some("call-1".into()),
            function: Some(FunctionCallStream {
                name: Some("fn".into()),
                arguments: Some("{}".into()),
            }),
            r#type: None,
        }];
        acc.process_tool_calls_delta(&chunks);
        let r = acc.finish();
        assert_eq!(r.tool_calls[0].id.as_deref(), Some("call-1"));
        assert_eq!(r.tool_calls[0].name, "fn");
        assert!(r.tool_calls[0].arguments.contains("{}"));
    }

    #[test]
    fn accumulator_processes_content_chunk() {
        let mut acc = StreamAccumulator::new(false);
        let sink = CountingSink::new();
        let mut resp = empty_stream_response();
        resp.choices.push(ChatChoiceStream {
            delta: ChatCompletionStreamResponseDelta {
                content: Some("hello".into()),
                ..delta_empty()
            },
            finish_reason: None,
            index: 0,
            logprobs: None,
        });
        let first = acc.process_chunk(resp, &sink, "think");
        assert!(first.is_some(), "first chunk should return Instant");
        let chunks = sink.chunks.lock().unwrap();
        assert_eq!(chunks[0].content, "hello");
        let r = acc.finish();
        assert_eq!(r.content, "hello");
    }

    #[test]
    fn accumulator_processes_tool_call_delta() {
        let mut acc = StreamAccumulator::new(false);
        let sink = CountingSink::new();
        let mut resp = empty_stream_response();
        resp.choices.push(ChatChoiceStream {
            delta: ChatCompletionStreamResponseDelta {
                tool_calls: Some(vec![ChatCompletionMessageToolCallChunk {
                    index: 0,
                    id: Some("c1".into()),
                    function: Some(FunctionCallStream {
                        name: Some("t".into()),
                        arguments: Some("{}".into()),
                    }),
                    r#type: None,
                }]),
                ..delta_empty()
            },
            finish_reason: None,
            index: 0,
            logprobs: None,
        });
        acc.process_chunk(resp, &sink, "think");
        let r = acc.finish();
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.tool_calls[0].name, "t");
    }

    #[test]
    fn accumulator_thinking_mode() {
        let mut acc = StreamAccumulator::new(true);
        let sink = CountingSink::new();
        let tag_s = crate::support::thinking::THINKING_START;
        let tag_e = crate::support::thinking::THINKING_END;
        let mut resp = empty_stream_response();
        resp.choices.push(ChatChoiceStream {
            delta: ChatCompletionStreamResponseDelta {
                content: Some(format!("a {}x{} b", tag_s, tag_e)),
                ..delta_empty()
            },
            finish_reason: None,
            index: 0,
            logprobs: None,
        });
        acc.process_chunk(resp, &sink, "think");
        acc.flush(&sink, "think");
        let mut saw_thinking = false;
        for c in sink.chunks.lock().unwrap().iter() {
            if c.kind == MessageChunkKind::Thinking {
                saw_thinking = true;
            }
        }
        assert!(saw_thinking);
        let r = acc.finish();
        assert_eq!(r.content, "a  b");
        assert_eq!(r.reasoning_content.as_deref(), Some("x"));
    }
}
