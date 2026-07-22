//! loom-llm: LLM client abstractions for Loom agents
//!
//! This crate provides the core LLM client traits, type definitions,
//! and implementations used by Loom's agent runtime.

pub mod client;
pub mod error;
pub mod factory;
pub mod message;
pub mod registry;
pub mod support;
pub mod tool;
pub mod traits;

#[cfg(test)]
mod test_util;

// Re-exports — Message types
pub use message::{
    assistant_content_for_chat_api, check_orphan_tool_calls, content_part_modality,
    message_summary, sanitize_tool_call_ids, AssistantPayload, AssistantToolCall, ContentError,
    ContentPart, Message, ToolCallContent, UserContent,
};

// Re-exports — Tool types (MCP-format ToolSpec, ToolCall, ToolSourceError, etc.)
pub use tool::{ToolCall, ToolOutputHint, ToolOutputStrategy, ToolSourceError, ToolSpec};

// Re-exports — Error types
#[allow(deprecated)]
pub use error::AgentError;
pub use error::LlmError;
pub use loom_graph_core::{GraphError, Interrupt};

// Re-exports — LLM traits and response types
pub use traits::{
    CompletionTokensDetails, LlmClient, LlmHeaders, LlmProvider, LlmResponse, LlmUsage,
    MessageChunk, MessageChunkKind, ModelCapabilities, ModelInfo, PromptTokensDetails, StreamSink,
    ToolChoiceMode,
};

// Re-exports — Client implementations
pub use client::{ChatOpenAI, ChatOpenAICompat, OpenAICompatProvider, OpenAIProvider};

// Re-exports — Registry types (ProviderConfig, ModelEntry, etc.)
pub use registry::{CachedModelList, CombinedModelList, ModelEntry, ProviderConfig};
