//! loom-llm: LLM client abstractions for Loom agents
//!
//! This crate provides the core LLM client traits and type definitions
//! used by Loom's agent runtime.

pub mod message;
pub mod tool;
pub mod error;
pub mod traits;
pub mod client;
pub mod registry;

// Re-exports
pub use message::{
    Message, UserContent, ContentPart, ContentError,
    AssistantToolCall, AssistantPayload, ToolCallContent,
    assistant_content_for_chat_api,
};

pub use tool::{ToolCall, ToolSpec, FunctionSpec};

pub use error::{AgentError, Interrupt, GraphInterrupt};

pub use traits::{
    LlmClient, LlmProvider, LlmResponse, LlmUsage, LlmHeaders,
    ToolChoiceMode, ToolCallDelta, ModelInfo, ModelCapabilities,
    PromptTokensDetails, CompletionTokensDetails,
    MessageChunk, MessageChunkKind, ProviderConfig, ModelEntry,
};

pub use client::ChatOpenAICompat;

pub use registry::ModelRegistry;