//! Stream response accumulation for OpenAI SSE chat completions.
//!
//! [`StreamAccumulator`] consumes `async_openai` stream chunks and
//! emits [`MessageChunk`] / [`ToolCallDelta`](crate::llm::ToolCallDelta) through channels, while
//! assembling the final [`LlmResponse`](crate::llm::LlmResponse) content.

use async_openai::types::chat::CreateChatCompletionStreamResponse;
use tokio::sync::mpsc;

use crate::llm::thinking::{collect_thinking_tags, strip_thinking_tags, ThinkingSegment, ThinkingTagParser};
use crate::llm::tool_call_accumulator::{RawToolCallDelta, ToolCallAccumulator};
use crate::llm::{LlmResponse, LlmUsage, ToolCallDelta};
use crate::stream::MessageChunk;

/// Accumulates streaming SSE chunks into a complete response.
pub(super) struct StreamAccumulator {
    full_content: String,
    tool_calls: ToolCallAccumulator,
    usage: Option<LlmUsage>,
    sent_any_content: bool,
    thinking_parser: Option<ThinkingTagParser>,
    parse_thinking_tags: bool,
}

pub(super) struct StreamResult {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub tool_calls: Vec<crate::state::ToolCall>,
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
        }
    }

    /// Process one SSE stream response chunk.
    ///
    /// Sends incremental content/thinking to `chunk_tx` and tool deltas
    /// to `tool_delta_tx`. Updates internal accumulators.
    pub async fn process_chunk(
        &mut self,
        response: CreateChatCompletionStreamResponse,
        chunk_tx: &mpsc::Sender<MessageChunk>,
        tool_delta_tx: Option<&mpsc::Sender<ToolCallDelta>>,
    ) {
        if let Some(ref u) = response.usage {
            self.usage = Some(LlmUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            });
        }

        for choice in response.choices {
            let delta = &choice.delta;

            if let Some(ref content) = delta.content {
                if !content.is_empty() {
                    self.full_content.push_str(content);
                    self.sent_any_content = true;

                    if let Some(ref mut parser) = self.thinking_parser {
                        for seg in parser.feed(content) {
                            match seg {
                                ThinkingSegment::Message(s) => {
                                    let _ = chunk_tx.send(MessageChunk::message(s)).await;
                                }
                                ThinkingSegment::Thinking(s) => {
                                    let _ = chunk_tx.send(MessageChunk::thinking(s)).await;
                                }
                            }
                        }
                    } else {
                        let _ = chunk_tx.send(MessageChunk::message(content.clone())).await;
                    }
                }
            }

            if let Some(ref tool_calls) = delta.tool_calls {
                for tc in tool_calls {
                    self.tool_calls.push(RawToolCallDelta {
                        index: tc.index,
                        id: tc.id.clone(),
                        name: tc.function.as_ref().and_then(|f| f.name.clone()),
                        arguments: tc.function.as_ref().and_then(|f| f.arguments.clone()),
                    });

                    if let Some(tool_tx) = tool_delta_tx {
                        let args_delta = tc
                            .function
                            .as_ref()
                            .and_then(|f| f.arguments.clone())
                            .unwrap_or_default();
                        if !args_delta.is_empty() || tc.id.is_some() {
                            let _ = tool_tx
                                .send(ToolCallDelta {
                                    call_id: tc.id.clone(),
                                    name: tc.function.as_ref().and_then(|f| f.name.clone()),
                                    arguments_delta: args_delta,
                                })
                                .await;
                        }
                    }
                }
            }
        }
    }

    /// Flush remaining thinking buffer and handle edge cases.
    ///
    /// Must be called after the stream ends, before `finish()`.
    pub async fn flush(&mut self, chunk_tx: &mpsc::Sender<MessageChunk>) {
        if let Some(parser) = self.thinking_parser.take() {
            if let Some(seg) = parser.flush() {
                match seg {
                    ThinkingSegment::Message(s) => {
                        let _ = chunk_tx.send(MessageChunk::message(s)).await;
                    }
                    ThinkingSegment::Thinking(s) => {
                        let _ = chunk_tx.send(MessageChunk::thinking(s)).await;
                    }
                }
            }
        }
    }

    /// True if no content and no tool calls were accumulated but usage
    /// indicates the model did generate tokens (proxy edge case).
    pub fn needs_fallback(&self) -> bool {
        let completion_tokens = self
            .usage
            .as_ref()
            .map(|u| u.completion_tokens)
            .unwrap_or(0);
        self.full_content.is_empty()
            && self.tool_calls.is_empty()
            && completion_tokens > 0
    }

    /// Replace accumulated data with a non-stream fallback response.
    pub async fn apply_fallback(
        &mut self,
        fallback: LlmResponse,
        chunk_tx: &mpsc::Sender<MessageChunk>,
    ) {
        self.full_content = fallback.content.clone();
        if !self.full_content.is_empty() {
            self.sent_any_content = true;
            let _ = chunk_tx
                .send(MessageChunk::message(self.full_content.clone()))
                .await;
        }
        if self.usage.is_none() {
            self.usage = fallback.usage;
        }
        self.tool_calls.replace_from_vec(fallback.tool_calls);
    }

    /// Send full content as one chunk if no incremental content was sent
    /// (some proxies only include content in the final payload).
    pub async fn emit_full_if_needed(&self, chunk_tx: &mpsc::Sender<MessageChunk>) {
        if !self.sent_any_content && !self.full_content.is_empty() {
            let _ = chunk_tx
                .send(MessageChunk::message(self.full_content.clone()))
                .await;
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

#[cfg(test)]
impl StreamAccumulator {
    fn test_empty_with_usage(usage: LlmUsage) -> Self {
        Self {
            full_content: String::new(),
            tool_calls: ToolCallAccumulator::new(),
            usage: Some(usage),
            sent_any_content: false,
            thinking_parser: None,
            parse_thinking_tags: false,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(deprecated)]

    use super::*;
    use async_openai::types::chat::{
        ChatChoiceStream, ChatCompletionMessageToolCallChunk, ChatCompletionStreamResponseDelta,
        CreateChatCompletionStreamResponse, FunctionCallStream,
    };
    use crate::stream::MessageChunkKind;

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

    #[tokio::test]
    async fn accumulator_processes_content_chunk() {
        let mut acc = StreamAccumulator::new(false);
        let (tx, mut rx) = mpsc::channel(8);
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
        acc.process_chunk(resp, &tx, None).await;
        let chunk = rx.recv().await.unwrap();
        assert_eq!(chunk.content, "hello");
        let r = acc.finish();
        assert_eq!(r.content, "hello");
    }

    #[tokio::test]
    async fn accumulator_processes_tool_call_delta() {
        let mut acc = StreamAccumulator::new(false);
        let (tx, _rx) = mpsc::channel(8);
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
        acc.process_chunk(resp, &tx, None).await;
        let r = acc.finish();
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.tool_calls[0].name, "t");
    }

    #[tokio::test]
    async fn accumulator_thinking_mode() {
        let mut acc = StreamAccumulator::new(true);
        let (tx, mut rx) = mpsc::channel(16);
        let tag_s = crate::llm::thinking::THINKING_START;
        let tag_e = crate::llm::thinking::THINKING_END;
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
        acc.process_chunk(resp, &tx, None).await;
        acc.flush(&tx).await;
        let mut saw_thinking = false;
        while let Ok(c) = rx.try_recv() {
            if c.kind == MessageChunkKind::Thinking {
                saw_thinking = true;
            }
        }
        assert!(saw_thinking);
        let r = acc.finish();
        assert_eq!(r.content, "a  b");
        assert_eq!(r.reasoning_content.as_deref(), Some("x"));
    }

    #[tokio::test]
    async fn accumulator_needs_fallback() {
        let acc = StreamAccumulator::test_empty_with_usage(LlmUsage {
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
        });
        assert!(acc.needs_fallback());
    }
}
