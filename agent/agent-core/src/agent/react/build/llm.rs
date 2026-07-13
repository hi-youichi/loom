//! Builds the default LLM from ReactBuildConfig (OpenAI or OpenAI-compat HTTP client).
//!
//! Only `LLM_PROVIDER=openai` uses the native `async_openai` client; all other providers
//! (including the default when the model prefix is not `openai/`) use `ChatOpenAICompat`.

use std::sync::Arc;

use loom_graph_core::GraphError;
use loom_llm::client::{FixedLlmProvider, RetryLlmClient};
use loom_llm::factory::create_llm_client;
use loom_llm::support::audit::LlmAuditLog;
use loom_llm::{ChatOpenAI, ChatOpenAICompat, LlmClient, LlmProvider, ModelEntry};
use model_spec_core::ModelTier;
use tool_core::ToolRegistryLocked;

use super::super::config::ReactBuildConfig;
use super::error::BuildRunnerError;

fn parse_provider_model(model: &str) -> Option<(&str, &str)> {
    let (provider, model_id) = model.split_once('/')?;
    let provider = provider.trim();
    let model_id = model_id.trim();
    if provider.is_empty() || model_id.is_empty() {
        return None;
    }
    Some((provider, model_id))
}

pub(crate) use model_spec_core::resolve_tier;
pub use model_spec_core::{DefaultTierResolver, ResolvedTierModel, TierResolver};

pub(crate) fn model_entry_from_config(
    config: &ReactBuildConfig,
) -> Result<ModelEntry, BuildRunnerError> {
    let api_key = config
        .openai_api_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            BuildRunnerError::Context(GraphError::ExecutionFailed(
                "OPENAI_API_KEY is not set".to_string(),
            ))
        })?;

    let base_url = config
        .openai_base_url
        .clone()
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok());

    tracing::debug!("🎯 Frontend config model: {:?}", config.model);

    let raw_model = config
        .model
        .as_deref()
        .filter(|m| !m.is_empty())
        .map(String::from)
        .or_else(|| std::env::var("MODEL").ok())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    tracing::info!("✅ Final model to use: {}", raw_model);

    tracing::debug!("🎯 Config provider type: {:?}", config.llm_provider);

    let inferred_provider_type = parse_provider_model(&raw_model).map(|(provider, _)| {
        if provider.eq_ignore_ascii_case("openai") {
            "openai".to_string()
        } else {
            "openai_compat".to_string()
        }
    });
    let _provider_type = config.llm_provider.clone().or(inferred_provider_type);

    let (provider_from_model, model) = parse_provider_model(&raw_model)
        .map(|(p, m)| (Some(p), m))
        .unwrap_or_else(|| (None, raw_model.as_str()));

    let provider = match config.llm_provider.as_deref().or(provider_from_model) {
        Some("bigmodel") => "bigmodel".to_string(),
        Some("openai_compat") => "openai_compat".to_string(),
        Some(other) => other.to_string(),
        None => "openai".to_string(),
    };

    let temperature = config
        .openai_temperature
        .clone()
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| match s.parse::<f32>() {
            Ok(v) if v.is_finite() => Some(v),
            _ => {
                tracing::warn!(
                    value = %s,
                    "ignoring invalid OPENAI_TEMPERATURE (expected a finite number)"
                );
                None
            }
        });

    Ok(ModelEntry {
        id: format!("{}/{}", provider, model),
        name: model.to_string(),
        provider,
        base_url,
        api_key: Some(api_key),
        provider_type: config.llm_provider.clone(),
        temperature,
        max_tokens: None,
        family: None,
        version: None,
    })
}

///
/// This is the async version that fetches tools from the tool source.
pub async fn build_default_llm_with_tool_source(
    config: &ReactBuildConfig,
    tool_source: &ToolRegistryLocked,
    audit_log: Option<Arc<dyn LlmAuditLog>>,
) -> Result<Box<dyn LlmClient>, BuildRunnerError> {
    let entry = model_entry_from_config(config)?;
    let provider_type = entry.provider_type.as_deref().unwrap_or_else(|| {
        if entry.provider.eq_ignore_ascii_case("openai") {
            "openai"
        } else {
            "openai_compat"
        }
    });

    let tools = tool_source.list_tools().await;

    match provider_type {
        "openai" => {
            let mut openai_config = async_openai::config::OpenAIConfig::new();
            if let Some(ref api_key) = entry.api_key {
                openai_config = openai_config.with_api_key(api_key);
            }
            if let Some(ref base_url) = entry.base_url {
                let base_url = base_url.trim_end_matches('/');
                openai_config = openai_config.with_api_base(base_url);
            }
            tracing::debug!("build_default_llm: OpenAI with tools");
            let mut client = ChatOpenAI::with_config(openai_config, entry.name).with_tools(tools);

            let trace_id = config
                .trace_thread_id
                .as_ref()
                .or(config.thread_id.as_ref());
            if let Some(thread_id) = trace_id {
                let headers = loom_llm::LlmHeaders::default().with_thread_id(thread_id);
                client = client.with_headers(headers);
                tracing::debug!("Set X-Thread-Id header: {}", thread_id);
            }

            if let Some(t) = entry.temperature {
                client = client.with_temperature(t);
            }
            if let Some(ref effort) = config.reasoning_effort {
                if effort != "auto" {
                    client = client.with_reasoning_effort(effort);
                }
            }
            if let Some(ref audit_log) = audit_log {
                client = client.with_audit_log(audit_log.clone());
            }
            let client: Box<dyn LlmClient> = Box::new(client);
            let retry_client = RetryLlmClient::new(Arc::from(client));
            Ok(Box::new(retry_client) as Box<dyn LlmClient>)
        }
        _ => {
            let base_url = entry
                .base_url
                .clone()
                .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
                .ok_or_else(|| {
                    BuildRunnerError::Context(GraphError::ExecutionFailed(format!(
                        "OPENAI_BASE_URL is required for non-openai provider '{}'",
                        provider_type
                    )))
                })?;
            let api_key = entry.api_key.clone().ok_or_else(|| {
                BuildRunnerError::Context(GraphError::ExecutionFailed(format!(
                    "api_key is required for provider '{}'",
                    provider_type
                )))
            })?;
            tracing::debug!(provider_type = %provider_type, "build_default_llm: OpenAI-compat with tools");
            let mut client =
                ChatOpenAICompat::with_config(base_url, api_key, entry.name).with_tools(tools);

            // Attach audit log if enabled
            if let Some(ref audit) = audit_log {
                client = client.with_audit_log(Arc::clone(audit));
            }

            let trace_id = config
                .trace_thread_id
                .as_ref()
                .or(config.thread_id.as_ref());
            if let Some(thread_id) = trace_id {
                let headers = loom_llm::LlmHeaders::default().with_thread_id(thread_id);
                client = client.with_headers(headers);
                tracing::debug!("Set X-Thread-Id header: {}", thread_id);
            }

            if let Some(t) = entry.temperature {
                client = client.with_temperature(t);
            }
            if let Some(ref effort) = config.reasoning_effort {
                if effort != "auto" {
                    client = client.with_reasoning_effort(effort);
                }
            }
            let client: Box<dyn LlmClient> = Box::new(client);
            let retry_client = RetryLlmClient::new(Arc::from(client));
            Ok(Box::new(retry_client) as Box<dyn LlmClient>)
        }
    }
}

pub(crate) async fn build_default_provider(
    config: &ReactBuildConfig,
    tool_source: &ToolRegistryLocked,
    audit_log: Option<Arc<dyn LlmAuditLog>>,
) -> Result<Arc<dyn LlmProvider>, BuildRunnerError> {
    let llm = build_default_llm_with_tool_source(config, tool_source, audit_log).await?;
    let entry = model_entry_from_config(config)?;
    Ok(Arc::new(FixedLlmProvider {
        client: Arc::from(llm),
        model_id: entry.name,
    }))
}

pub(crate) async fn resolve_title_provider(
    config: &ReactBuildConfig,
) -> Option<Arc<dyn LlmProvider>> {
    let providers = env_config::load_provider_configs_from_xdg().unwrap_or_default();
    let provider_hint = crate::agent::react::tier_apply::extract_provider_hint(config);
    let resolved = resolve_tier(
        config.model.as_deref(),
        ModelTier::Light,
        provider_hint.as_deref(),
        &providers,
    )
    .await?;
    let (provider, model_name) = ModelEntry::parse_id(&resolved.model_id)?;
    let entry = ModelEntry {
        id: resolved.model_id.clone(),
        name: model_name.to_string(),
        provider: provider.to_string(),
        base_url: resolved.base_url,
        api_key: resolved.api_key,
        provider_type: resolved.provider_type,
        temperature: None,
        max_tokens: None,
        family: None,
        version: None,
    };
    let thread_id = config
        .trace_thread_id
        .as_ref()
        .or(config.thread_id.as_ref());
    let headers = thread_id.map(|tid| loom_llm::LlmHeaders::default().with_thread_id(tid));
    let client = create_llm_client(&entry, headers).ok()?;
    Some(Arc::new(FixedLlmProvider {
        client: Arc::from(client),
        model_id: entry.name,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn restore_env(key: &str, old: Option<String>) {
        match old {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn model_provider_prefix_non_openai_infers_openai_compat_and_normalizes_model() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        let old_base = std::env::var("OPENAI_BASE_URL").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::set_var("OPENAI_BASE_URL", "https://open.bigmodel.cn/api/paas/v4");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("zhipuai-coding-plan/glm-5".to_string());
        config.llm_provider = Some("openai_compat".to_string());

        let entry = model_entry_from_config(&config).unwrap();

        if let Some(key) = old_key {
            std::env::set_var("OPENAI_API_KEY", key);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }
        if let Some(base) = old_base {
            std::env::set_var("OPENAI_BASE_URL", base);
        } else {
            std::env::remove_var("OPENAI_BASE_URL");
        }
        drop(_guard);

        assert_eq!(entry.provider_type.as_deref(), Some("openai_compat"));
        assert_eq!(entry.provider, "openai_compat");
        assert_eq!(entry.name, "glm-5");
    }

    #[test]
    fn model_provider_prefix_openai_infers_openai_and_normalizes_model() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        let old_base = std::env::var("OPENAI_BASE_URL").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::set_var("OPENAI_BASE_URL", "https://api.openai.com/v1");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("openai/gpt-4o".to_string());
        config.llm_provider = Some("openai".to_string());

        let entry = model_entry_from_config(&config).unwrap();

        if let Some(key) = old_key {
            std::env::set_var("OPENAI_API_KEY", key);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }
        if let Some(base) = old_base {
            std::env::set_var("OPENAI_BASE_URL", base);
        } else {
            std::env::remove_var("OPENAI_BASE_URL");
        }
        drop(_guard);

        assert_eq!(entry.provider_type.as_deref(), Some("openai"));
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.name, "gpt-4o");
    }

    #[test]
    fn test_model_entry_from_react_build_config() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        let old_base = std::env::var("OPENAI_BASE_URL").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::set_var("OPENAI_BASE_URL", "https://api.openai.com/v1");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("openai/gpt-4o".to_string());
        config.llm_provider = Some("openai".to_string());

        let entry = model_entry_from_config(&config).unwrap();

        if let Some(key) = old_key {
            std::env::set_var("OPENAI_API_KEY", key);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }
        if let Some(base) = old_base {
            std::env::set_var("OPENAI_BASE_URL", base);
        } else {
            std::env::remove_var("OPENAI_BASE_URL");
        }
        drop(_guard);

        assert_eq!(entry.provider_type.as_deref(), Some("openai"));
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.name, "gpt-4o");
    }

    #[test]
    fn explicit_provider_type_overrides_model_provider_inference() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("zhipuai-coding-plan/glm-5".to_string());
        config.llm_provider = Some("openai".to_string());
        let entry = model_entry_from_config(&config).unwrap();

        restore_env("OPENAI_API_KEY", old_key);
        drop(_guard);

        assert_eq!(entry.provider_type.as_deref(), Some("openai"));
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.name, "glm-5");
    }

    #[test]
    fn inferred_openai_compat_requires_base_url() {
        let _guard = env_lock().lock().unwrap();
        let old_key = std::env::var("OPENAI_API_KEY").ok();
        let old_base = std::env::var("OPENAI_BASE_URL").ok();
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::remove_var("OPENAI_BASE_URL");

        let mut config = crate::agent::react::config::ReactBuildConfig::from_env();
        config.model = Some("zhipuai-coding-plan/glm-5".to_string());
        config.llm_provider = Some("openai_compat".to_string());
        let entry = model_entry_from_config(&config).unwrap();

        restore_env("OPENAI_API_KEY", old_key);
        restore_env("OPENAI_BASE_URL", old_base);
        drop(_guard);

        // When provider_type is set to openai_compat, it should succeed even without base_url
        assert_eq!(entry.provider_type.as_deref(), Some("openai_compat"));
        assert_eq!(entry.provider, "openai_compat");
        assert_eq!(entry.name, "glm-5");
    }
}
