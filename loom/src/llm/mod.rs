//! LLM client module for Loom.
//!
//! This module contains Loom's LLM client abstractions.

pub use loom_llm::message::{
    Message, UserContent, ContentPart, ContentError,
    AssistantToolCall, AssistantPayload, ToolCallContent,
    assistant_content_for_chat_api,
};

pub use loom_llm::tool::{ToolCall, ToolSpec};

pub use loom_llm::traits::{
    LlmClient, LlmProvider, LlmResponse, LlmUsage, LlmHeaders,
    ToolChoiceMode, ToolCallDelta, ModelInfo, ModelCapabilities,
    PromptTokensDetails, CompletionTokensDetails,
    MessageChunk,
};

// Loom-specific LLM submodules (these have their own ModelEntry, ProviderConfig)
pub mod audit;
pub mod error_classifier;
pub mod model_registry;
pub mod thinking;
pub mod tool_call_accumulator;

mod factory;

// Loom-specific LLM implementations
mod openai;
mod openai_provider;
mod openai_compat;
mod openai_compat_provider;
mod fixed_provider;
mod mock;
mod retry;

pub use openai::ChatOpenAI;
pub use openai_provider::OpenAIProvider;
pub use openai_compat::ChatOpenAICompat;
pub use openai_compat_provider::OpenAICompatProvider;
pub use fixed_provider::FixedLlmProvider;
pub use mock::{MockLlm, MultiRoundMockLlm};
pub use retry::RetryLlmClient;

// Re-export factory and registry types from local modules
pub use factory::LlmFactory;
pub use model_registry::{ModelEntry, ProviderConfig, ModelRegistry, create_llm_client};

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
