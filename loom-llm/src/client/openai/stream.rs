//! Stream processing for OpenAI Chat Completions.

use async_openai::types::chat::ChatCompletionResponseStream;
use tokio::sync::mpsc;

use crate::types::message::AssistantToolCall;
use crate::traits::{ToolCallDelta, MessageChunk};

/// Accumulates streamed chunks into a complete response.
pub struct StreamAccumulator {
    /// Whether to parse thinking tags.
    parse_thinking_tags: bool,
    /// Accumulated content text.
    content: String,
    /// Accumulated reasoning/thinking text.
    reasoning_content: Option<String>,
    /// Accumulated tool calls.
    tool_calls: Vec<AssistantToolCall>,
    /// Current tool call accumulator for incremental parsing.
    current_tool_call: Option<ToolCallAccumulator>,
    /// Usage information (may come in final chunk).
    usage: Option<UsageInfo>,
}

struct UsageInfo {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

struct ToolCallAccumulator {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl StreamAccumulator {
    /// Creates a new stream accumulator.
    pub fn new(parse_thinking_tags: bool) -> Self {
        Self {
            parse_thinking_tags,
            content: String::new(),
            reasoning_content: None,
            tool_calls: Vec::new(),
            current_tool_call: None,
            usage: None,
        }
    }

    /// Process a single streamed chunk.
    pub async fn process_chunk(
        &mut self,
        chunk: ChatCompletionResponseStream,
        _chunk_tx: &mpsc::Sender<MessageChunk>,
        _tool_delta_tx: Option<&mpsc::Sender<ToolCallDelta>>,
    ) {
        for choice in chunk.choices {
            if let Some(delta) = choice.delta {
                // Handle content delta
                if let Some(content) = delta.content {
                    self.content.push_str(&content);
                }

                // Handle reasoning content (if present)
                if let Some(reasoning) = delta.reasoning_content {
                    let rc = self.reasoning_content.get_or_insert_with(String::new);
                    rc.push_str(&reasoning);
                }

                // Handle tool calls
                if let Some(tool_calls) = delta.tool_calls {
                    for tc_delta in tool_calls {
                        self.process_tool_call_delta(tc_delta);
                    }
                }
            }

            // Handle usage in chunk (some providers send this)
            if let Some(usage) = choice.usage {
                self.usage = Some(UsageInfo {
                    prompt_tokens: usage.prompt_tokens,
                    completion_tokens: usage.completion_tokens,
                    total_tokens: usage.total_tokens,
                });
            }
        }
    }

    fn process_tool_call_delta(&mut self, delta: serde_json::Value) {
        // Parse the delta for tool call updates
        if let Some(index) = delta.get("index").and_then(|v| v.as_u64()) {
            let index = index as usize;
            
            // Ensure we have enough tool call slots
            while self.tool_calls.len() <= index {
                self.tool_calls.push(AssistantToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
            }

            if let Some(function) = delta.get("function") {
                if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                    self.tool_calls[index].name.push_str(name);
                }
                if let Some(args) = function.get("arguments").and_then(|v| v.as_str()) {
                    self.tool_calls[index].arguments.push_str(args);
                }
            }
            if let Some(id) = delta.get("id").and_then(|v| v.as_str()) {
                self.tool_calls[index].id.push_str(id);
            }
        }
    }

    /// Flush any buffered content.
    pub async fn flush(&mut self, _chunk_tx: &mpsc::Sender<MessageChunk>) {
        // No-op for now; content is accumulated directly
    }

    /// Emit full content if needed (for empty responses).
    pub async fn emit_full_if_needed(&mut self, _chunk_tx: &mpsc::Sender<MessageChunk>) {
        // Content is already accumulated
    }

    /// Finish and return the complete response.
    pub fn finish(self) -> StreamResult {
        StreamResult {
            content: self.content,
            reasoning_content: self.reasoning_content,
            tool_calls: self.tool_calls,
            usage: self.usage.map(|u| crate::traits::LlmUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        }
    }
}

/// Result of stream processing.
pub struct StreamResult {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub tool_calls: Vec<AssistantToolCall>,
    pub usage: Option<crate::traits::LlmUsage>,
}