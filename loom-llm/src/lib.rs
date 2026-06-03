//! loom-llm: LLM client abstractions for Loom agents
//!
//! This crate provides the core LLM client traits, type definitions,
//! and implementations used by Loom's agent runtime.

pub mod message;
pub mod tool;
pub mod error;
pub mod traits;
pub mod client;
pub mod registry;
pub mod support;

// Re-exports — Message types
pub use message::{
    Message, UserContent, ContentPart, ContentError,
    AssistantToolCall, AssistantPayload, ToolCallContent,
    assistant_content_for_chat_api,
};

// Re-exports — Tool types (MCP-format ToolSpec, ToolCall, ToolSourceError, etc.)
pub use tool::{
    ToolCall, ToolSpec, ToolOutputHint, ToolOutputStrategy,
    ToolSourceError,
};

// Re-exports — Error types
pub use error::{AgentError, Interrupt, GraphInterrupt};

// Re-exports — LLM traits and response types
pub use traits::{
    LlmClient, LlmProvider, LlmResponse, LlmUsage, LlmHeaders,
    ToolChoiceMode, ToolCallDelta, ModelInfo, ModelCapabilities,
    PromptTokensDetails, CompletionTokensDetails,
    MessageChunk, MessageChunkKind,
};

// Re-exports — Client implementations
pub use client::ChatOpenAICompat;

// Re-exports — Registry types (ProviderConfig, ModelEntry, etc.)
pub use registry::{ProviderConfig, ModelEntry, CachedModelList, CombinedModelList};
