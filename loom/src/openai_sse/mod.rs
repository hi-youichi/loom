//! OpenAI-compatible Chat Completions SSE adapter — re-exported from loom-stream.
//!
//! The chunk/request types and StreamToSse adapter live in `loom-stream::openai_sse`.
//! This module re-exports them and adds `parse_chat_request` which depends on helve/memory.

// Re-export everything from loom-stream
pub use loom_stream::openai_sse::{
    ChatCompletionChunk, ChatCompletionRequest, ChatMessage, ChunkChoice, ChunkMeta, ChunkUsage,
    Delta, DeltaToolCall, DeltaToolCallFunction, MessageContent, StreamOptions, StreamToSse,
    write_sse_line,
};

mod parse;

pub use parse::{parse_chat_request, ParseError, ParsedChatRequest};
