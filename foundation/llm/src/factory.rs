//! LLM client factory functions.
//!
//! `create_llm_client` and `create_llm_provider` create concrete LLM clients
//! (ChatOpenAI / ChatOpenAICompat) from a `ModelEntry`.
//!
//! These functions live here (rather than in `loom-tier` or `loom`) because they
//! only depend on `loom_llm` types + `model_spec_core` + `loom_graph_core`, and both
//! `agent-core` and `loom` need to call them without creating a circular dependency.

use std::sync::Arc;

use async_openai::config::OpenAIConfig;

use crate::traits::{LlmClient, LlmHeaders, LlmProvider};
use crate::{ChatOpenAI, ChatOpenAICompat, OpenAICompatProvider, OpenAIProvider};
use loom_graph_core::GraphError;
use model_spec_core::registry::ModelEntry;

/// Creates an LLM client from a ModelEntry with provider configuration.
pub fn create_llm_client(
    entry: &ModelEntry,
    headers: Option<LlmHeaders>,
) -> Result<Box<dyn LlmClient>, GraphError> {
    let model = entry.name.clone();
    let provider_type = entry.provider_type.as_deref().unwrap_or_else(|| {
        if entry.provider.eq_ignore_ascii_case("openai") {
            "openai"
        } else {
            "openai_compat"
        }
    });

    let client: Box<dyn LlmClient> = match provider_type {
        "openai" => {
            let mut config = OpenAIConfig::new();
            if let Some(ref api_key) = entry.api_key {
                config = config.with_api_key(api_key);
            }
            if let Some(ref base_url) = entry.base_url {
                let base_url = base_url.trim_end_matches('/');
                config = config.with_api_base(base_url);
            }
            let mut client = ChatOpenAI::with_config(config, model);
            if let Some(ref h) = headers {
                client = client.with_headers(h.clone());
            }
            if let Some(temp) = entry.temperature {
                client = client.with_temperature(temp);
            }
            Box::new(client)
        }
        _ => {
            let api_key = entry.api_key.clone().ok_or_else(|| {
                GraphError::ExecutionFailed(format!(
                    "api_key is required for provider '{}'",
                    provider_type
                ))
            })?;
            let base_url = entry
                .base_url
                .clone()
                .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
                .ok_or_else(|| {
                    GraphError::ExecutionFailed(format!(
                        "base_url (or OPENAI_BASE_URL) is required for non-openai provider '{}'",
                        provider_type
                    ))
                })?;
            let mut client = ChatOpenAICompat::with_config(base_url, api_key, model);
            if let Some(ref h) = headers {
                client = client.with_headers(h.clone());
            }
            if let Some(temp) = entry.temperature {
                client = client.with_temperature(temp);
            }
            Box::new(client)
        }
    };

    Ok(client)
}

/// Create an [`LlmProvider`] from a [`ModelEntry`].
pub fn create_llm_provider(
    entry: &ModelEntry,
) -> Result<Arc<dyn LlmProvider>, GraphError> {
    let provider_type = entry.provider_type.as_deref().unwrap_or_else(|| {
        if entry.provider.eq_ignore_ascii_case("openai") {
            "openai"
        } else {
            "openai_compat"
        }
    });

    match provider_type {
        "openai" => {
            let provider = OpenAIProvider::from_entry(entry);
            Ok(Arc::new(provider))
        }
        _ => {
            let provider = OpenAICompatProvider::from_entry(entry);
            Ok(Arc::new(provider))
        }
    }
}
