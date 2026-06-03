//! LLM client module for Loom.
//!
//! This module re-exports all LLM types and implementations from `loom-llm`.
//! Only `model_registry` (runtime) and `factory` stay here because they depend
//! on `crate::model_spec::ModelsDevResolver` and `crate::provider`/`crate::tier`.

// Re-export core types from loom-llm
pub use loom_llm::message::{
    Message, UserContent, ContentPart, ContentError,
    AssistantToolCall, AssistantPayload, ToolCallContent,
    assistant_content_for_chat_api,
};

pub use loom_llm::tool::{ToolCall, ToolSpec, ToolOutputHint, ToolOutputStrategy, ToolSourceError};

pub use loom_llm::traits::{
    LlmClient, LlmProvider, LlmResponse, LlmUsage, LlmHeaders,
    ToolChoiceMode, ToolCallDelta, ModelInfo, ModelCapabilities,
    PromptTokensDetails, CompletionTokensDetails,
    MessageChunk,
};

// Re-export support modules from loom-llm
pub use loom_llm::support::thinking;
pub use loom_llm::support::tool_call_accumulator;
pub use loom_llm::support::error_classifier;
pub use loom_llm::support::audit;

// Re-export client implementations from loom-llm (all implementations now live there)
pub use loom_llm::client::{
    ChatOpenAI, ChatOpenAICompat,
    OpenAIProvider, OpenAICompatProvider,
    RetryLlmClient, MockLlm, MultiRoundMockLlm, FixedLlmProvider,
};

// Local modules that depend on loom crate internals
pub mod model_registry;
mod factory;

// Re-export factory
pub use factory::LlmFactory;

// Re-export registry types from model_registry (data types come from loom-llm, runtime stays here)
pub use model_registry::{ModelEntry, ProviderConfig, ModelRegistry, create_llm_client, create_llm_provider};

#[deprecated(note = "renamed to ChatOpenAICompat")]
pub type ChatBigModel = ChatOpenAICompat;

pub fn get_headers_from_env() -> LlmHeaders {
    LlmHeaders {
        thread_id: std::env::var("LLM_THREAD_ID").ok(),
        trace_id: std::env::var("LLM_TRACE_ID").ok(),
        custom_headers: std::collections::HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_choice_mode_from_str() {
        assert_eq!("auto".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::Auto);
        assert_eq!("none".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::None);
        assert_eq!("required".parse::<ToolChoiceMode>().unwrap(), ToolChoiceMode::Required);
    }

    #[test]
    fn test_llm_headers() {
        let headers = LlmHeaders::default()
            .with_thread_id("test-thread")
            .with_trace_id("test-trace");
        assert_eq!(headers.thread_id, Some("test-thread".to_string()));
        assert_eq!(headers.trace_id, Some("test-trace".to_string()));
    }
}
