//! Models.dev resolver: fetch complete model metadata from https://models.dev/api.json

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::http_retry::{
    is_retryable_reqwest_error, retry_backoff_for_attempt, TRANSIENT_HTTP_MAX_RETRIES,
};

use super::resolver::ModelLimitResolver;
use super::spec::{Cost, Model, ModelLimit, ModelSpec, Modalities, ModalityType, Provider};

/// Default models.dev API URL.
pub const DEFAULT_MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// Fetches JSON from a URL. Abstraction for testing.
#[async_trait]
pub trait HttpClient: Send + Sync {
    /// GET the URL and return the response body as string.
    async fn get(&self, url: &str) -> Result<String, String>;
}

/// Reqwest-based HTTP client.
pub struct ReqwestHttpClient;

#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn get(&self, url: &str) -> Result<String, String> {
        let client = reqwest::Client::new();
        let mut attempt = 0;
        loop {
            let response = match client.get(url).send().await {
                Ok(response) => response,
                Err(e)
                    if is_retryable_reqwest_error(&e) && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "models.dev request failed, retrying"
                    );
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            };
            let response = response.error_for_status().map_err(|e| e.to_string())?;
            match response.text().await {
                Ok(body) => return Ok(body),
                Err(e)
                    if is_retryable_reqwest_error(&e) && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "models.dev response read failed, retrying"
                    );
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

/// Resolves complete model metadata from models.dev API.
pub struct ModelsDevResolver {
    base_url: String,
    http_client: Arc<dyn HttpClient>,
}

impl ModelsDevResolver {
    /// Create with default URL and reqwest client.
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_MODELS_DEV_URL.to_string(),
            http_client: Arc::new(ReqwestHttpClient),
        }
    }

    /// Create with custom URL and HTTP client.
    pub fn with_client(base_url: String, http_client: Arc<dyn HttpClient>) -> Self {
        Self {
            base_url,
            http_client,
        }
    }

    /// Fetch full JSON and parse into provider -> model_id -> ModelSpec map.
    /// Key format: "provider_id/model_id".
    pub async fn fetch_all(&self) -> Result<HashMap<String, ModelSpec>, String> {
        let body = self.http_client.get(&self.base_url).await?;
        parse_all_models(&body)
    }
    
    /// Fetch all providers with complete metadata.
    pub async fn fetch_all_providers(&self) -> Result<HashMap<String, Provider>, String> {
        let body = self.http_client.get(&self.base_url).await?;
        parse_all_providers(&body)
    }
    
    /// Fetch single provider with complete metadata.
    pub async fn fetch_provider(&self, provider_id: &str) -> Option<Provider> {
        let body = self.http_client.get(&self.base_url).await.ok()?;
        let json: Value = serde_json::from_str(&body).ok()?;
        let provider_json = json.get(provider_id)?;
        parse_provider(provider_id, provider_json)
    }
    
    /// Fetch single model with complete metadata.
    pub async fn fetch_model(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        let provider = self.fetch_provider(provider_id).await?;
        provider.models.get(model_id).cloned()
    }

    fn resolve_from_json(
        &self,
        json: &Value,
        provider_id: &str,
        model_id: &str,
    ) -> Option<ModelSpec> {
        let provider = json.get(provider_id)?;
        let models = provider.get("models")?.as_object()?;

        // Try model_id as-is first (e.g. "openai/gpt-5")
        let model = models.get(model_id).or_else(|| {
            // Try "provider_id/model_id" if model_id has no slash
            if !model_id.contains('/') {
                models.get(&format!("{}/{}", provider_id, model_id))
            } else {
                None
            }
        })?;

        parse_model_spec(model)
    }
}

impl Default for ModelsDevResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModelLimitResolver for ModelsDevResolver {
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<ModelSpec> {
        let body = self.http_client.get(&self.base_url).await.ok()?;
        let json: Value = serde_json::from_str(&body).ok()?;
        self.resolve_from_json(&json, provider_id, model_id)
    }
}

/// Parse Provider from JSON
fn parse_provider(provider_id: &str, value: &Value) -> Option<Provider> {
    let name = value.get("name").and_then(|v| v.as_str())?.to_string();
    
    let env = value
        .get("env")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    
    let npm = value
        .get("npm")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    
    let doc = value
        .get("doc")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    
    let models = value
        .get("models")
        .and_then(|v| v.as_object())?
        .iter()
        .filter_map(|(model_id, model_value)| {
            parse_model(model_id, model_value).map(|model| (model_id.clone(), model))
        })
        .collect();
    
    Some(Provider {
        id: provider_id.to_string(),
        name,
        env,
        npm,
        doc,
        models,
    })
}

/// Parse all providers from JSON
fn parse_all_providers(body: &str) -> Result<HashMap<String, Provider>, String> {
    let json: Value = serde_json::from_str(body)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;
    
    let json_obj = json
        .as_object()
        .ok_or_else(|| "JSON is not an object".to_string())?;
    
    let mut providers = HashMap::new();
    
    for (provider_id, provider_value) in json_obj {
        if let Some(provider) = parse_provider(provider_id, provider_value) {
            providers.insert(provider_id.clone(), provider);
        }
    }
    
    Ok(providers)
}

/// Parse Model from JSON
fn parse_model(model_id: &str, value: &Value) -> Option<Model> {
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(model_id)
        .to_string();
    
    let family = value
        .get("family")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    
    let attachment = value
        .get("attachment")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let reasoning = value
        .get("reasoning")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let tool_call = value
        .get("tool_call")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let temperature = value
        .get("temperature")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    
    let structured_output = value
        .get("structured_output")
        .and_then(|v| v.as_bool());
    
    let knowledge = value
        .get("knowledge")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    
    let release_date = value
        .get("release_date")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    
    let last_updated = value
        .get("last_updated")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    
    let modalities = value
        .get("modalities")
        .map(parse_modalities)
        .unwrap_or_default();
    
    let open_weights = value
        .get("open_weights")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    let cost = value.get("cost").and_then(parse_cost);
    
    let limit = value.get("limit").and_then(parse_model_limit);
    
    Some(Model {
        id: model_id.to_string(),
        name,
        family,
        attachment,
        reasoning,
        tool_call,
        temperature,
        structured_output,
        knowledge,
        release_date,
        last_updated,
        modalities,
        open_weights,
        cost,
        limit,
    })
}

/// Parse ModelSpec from a model JSON object
pub(super) fn parse_model_spec(model: &Value) -> Option<ModelSpec> {
    // Try to parse complete model, fallback to just limit for backward compatibility
    let full_model = parse_model("", model);
    
    if let Some(model) = full_model {
        ModelSpec::from_model(&model)
    } else {
        // Fallback: only limit field is present
        let limit = parse_model_limit(model)?;
        Some(ModelSpec::from_limit(&limit))
    }
}

/// Parse Modalities from JSON
fn parse_modalities(value: &Value) -> Modalities {
    let input = value
        .get("input")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str().and_then(|s| match s {
                        "text" => Some(ModalityType::Text),
                        "image" => Some(ModalityType::Image),
                        "audio" => Some(ModalityType::Audio),
                        "video" => Some(ModalityType::Video),
                        "pdf" => Some(ModalityType::Pdf),
                        _ => None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    
    let output = value
        .get("output")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str().and_then(|s| match s {
                        "text" => Some(ModalityType::Text),
                        "image" => Some(ModalityType::Image),
                        "audio" => Some(ModalityType::Audio),
                        "video" => Some(ModalityType::Video),
                        "pdf" => Some(ModalityType::Pdf),
                        _ => None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    
    Modalities { input, output }
}

/// Parse Cost from JSON (costs are in USD per 1M tokens)
fn parse_cost(value: &Value) -> Option<Cost> {
    let input = value
        .get("input")
        .and_then(|v| v.as_f64())
        .map(|v| (v * 100.0) as u32)?;
    
    let output = value
        .get("output")
        .and_then(|v| v.as_f64())
        .map(|v| (v * 100.0) as u32)?;
    
    let cache_read = value
        .get("cache_read")
        .and_then(|v| v.as_f64())
        .map(|v| (v * 100.0) as u32);
    
    let cache_write = value
        .get("cache_write")
        .and_then(|v| v.as_f64())
        .map(|v| (v * 100.0) as u32);
    
    Some(Cost {
        input,
        output,
        cache_read,
        cache_write,
    })
}

/// Parse ModelLimit from JSON
pub(crate) fn parse_model_limit(limit: &Value) -> Option<ModelLimit> {
    let context = limit.get("context")?.as_u64()? as u32;
    let output = limit.get("output")?.as_u64()? as u32;

    let cache_read = limit
        .get("cache_read")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    
    let cache_write = limit
        .get("cache_write")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    Some(ModelLimit {
        context,
        output,
        cache_read,
        cache_write,
    })
}

/// Parse all models into ModelSpec map (legacy format for backward compatibility)
fn parse_all_models(body: &str) -> Result<HashMap<String, ModelSpec>, String> {
    let json: Value = serde_json::from_str(body)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;
    
    let json_obj = json
        .as_object()
        .ok_or_else(|| "JSON is not an object".to_string())?;
    
    let mut specs = HashMap::new();
    
    for (provider_id, provider_value) in json_obj {
        if let Some(models) = provider_value.get("models").and_then(|v| v.as_object()) {
            for (model_id, model_value) in models {
                if let Some(spec) = parse_model_spec(model_value) {
                    let key = format!("{}/{}", provider_id, model_id);
                    specs.insert(key, spec);
                }
            }
        }
    }
    
    Ok(specs)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockHttpClient {
        body: String,
    }

    #[async_trait]
    impl HttpClient for MockHttpClient {
        async fn get(&self, _url: &str) -> Result<String, String> {
            Ok(self.body.clone())
        }
    }

    fn fixture_json() -> String {
        r#"{
            "anthropic": {
                "id": "anthropic",
                "name": "Anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "npm": "@ai-sdk/anthropic",
                "doc": "https://docs.anthropic.com",
                "models": {
                    "claude-3-5-sonnet-20241022": {
                        "id": "claude-3-5-sonnet-20241022",
                        "name": "Claude Sonnet 3.5 v2",
                        "family": "claude-sonnet",
                        "attachment": true,
                        "reasoning": false,
                        "tool_call": true,
                        "temperature": true,
                        "knowledge": "2024-04-30",
                        "modalities": {
                            "input": ["text", "image", "pdf"],
                            "output": ["text"]
                        },
                        "open_weights": false,
                        "cost": {
                            "input": 3,
                            "output": 15,
                            "cache_read": 0.3,
                            "cache_write": 3.75
                        },
                        "limit": {
                            "context": 200000,
                            "output": 8192
                        }
                    }
                }
            },
            "zenmux": {
                "models": {
                    "openai/gpt-5": {
                        "limit": { "context": 400000, "output": 64000 }
                    },
                    "anthropic/claude-sonnet-4": {
                        "limit": { "context": 1000000, "output": 64000 }
                    }
                }
            }
        }"#
        .to_string()
    }

    #[tokio::test]
    async fn resolve_by_provider_and_model_id() {
        let client = Arc::new(MockHttpClient {
            body: fixture_json(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);

        let spec = resolver.resolve("anthropic", "claude-3-5-sonnet-20241022").await.unwrap();
        assert_eq!(spec.context_limit, 200_000);
        assert_eq!(spec.output_limit, 8192);
        assert!(spec.supports_vision());
        assert!(spec.supports_pdf());
        assert!(!spec.supports_audio());
    }

    #[tokio::test]
    async fn resolve_returns_none_for_unknown_model() {
        let client = Arc::new(MockHttpClient {
            body: fixture_json(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);

        assert!(resolver.resolve("zenmux", "unknown-model").await.is_none());
        assert!(resolver
            .resolve("unknown-provider", "gpt-5")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn fetch_all_parses_all_providers() {
        let client = Arc::new(MockHttpClient {
            body: fixture_json(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);

        let all = resolver.fetch_all().await.unwrap();
        assert!(all.contains_key("anthropic/claude-3-5-sonnet-20241022"));
        assert!(all.contains_key("zenmux/openai/gpt-5"));
    }

    #[tokio::test]
    async fn fetch_provider_with_complete_metadata() {
        let client = Arc::new(MockHttpClient {
            body: fixture_json(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        
        let provider = resolver.fetch_provider("anthropic").await.unwrap();
        
        assert_eq!(provider.id, "anthropic");
        assert_eq!(provider.name, "Anthropic");
        assert_eq!(provider.env, vec!["ANTHROPIC_API_KEY"]);
        assert_eq!(provider.npm, Some("@ai-sdk/anthropic".to_string()));
        assert!(provider.models.contains_key("claude-3-5-sonnet-20241022"));
    }

    #[tokio::test]
    async fn fetch_model_with_complete_metadata() {
        let client = Arc::new(MockHttpClient {
            body: fixture_json(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        
        let model = resolver.fetch_model("anthropic", "claude-3-5-sonnet-20241022").await.unwrap();
        
        assert_eq!(model.id, "claude-3-5-sonnet-20241022");
        assert_eq!(model.name, "Claude Sonnet 3.5 v2");
        assert_eq!(model.family, Some("claude-sonnet".to_string()));
        assert!(model.attachment);
        assert!(!model.reasoning);
        assert!(model.tool_call);
        assert!(model.temperature);
        assert_eq!(model.knowledge, Some("2024-04-30".to_string()));
        
        assert!(model.modalities.supports_vision());
        assert!(model.modalities.supports_pdf());
        assert!(!model.modalities.supports_audio());
        
        let cost = model.cost.unwrap();
        assert_eq!(cost.input_cost_usd(), 3.0);
        assert_eq!(cost.output_cost_usd(), 15.0);
        
        let limit = model.limit.unwrap();
        assert_eq!(limit.context, 200_000);
        assert_eq!(limit.output, 8192);
    }

    #[test]
    fn new_uses_default_url_and_reqwest_client() {
        let resolver = ModelsDevResolver::new();
        assert_eq!(resolver.base_url, DEFAULT_MODELS_DEV_URL);
    }

    #[tokio::test]
    async fn resolve_returns_none_when_http_client_fails() {
        struct FailingHttpClient;
        #[async_trait]
        impl HttpClient for FailingHttpClient {
            async fn get(&self, _url: &str) -> Result<String, String> {
                Err("network error".to_string())
            }
        }
        let resolver = ModelsDevResolver::with_client(
            "https://example.com/api.json".to_string(),
            Arc::new(FailingHttpClient),
        );
        assert!(resolver.resolve("zenmux", "openai/gpt-5").await.is_none());
    }

    #[tokio::test]
    async fn resolve_returns_none_when_json_is_invalid() {
        let client = Arc::new(MockHttpClient {
            body: "not valid json {{{".to_string(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        assert!(resolver.resolve("zenmux", "openai/gpt-5").await.is_none());
    }

    #[tokio::test]
    async fn resolve_fallback_when_model_id_has_no_slash() {
        // Provider "zai" has model key "zai
        let json = r#"{"zai":{"models":{"glm-5":{"limit":{"context":204800,"output":131072}}}}}"#;
        let client = Arc::new(MockHttpClient {
            body: json.to_string(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        let spec = resolver.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(spec.context_limit, 204_800);
        assert_eq!(spec.output_limit, 131_072);
    }

    #[tokio::test]
    async fn fetch_all_providers_returns_complete_metadata() {
        let client = Arc::new(MockHttpClient {
            body: fixture_json(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        
        let providers = resolver.fetch_all_providers().await.unwrap();
        assert!(providers.contains_key("anthropic"));
        
        let anthropic = providers.get("anthropic").unwrap();
        assert_eq!(anthropic.name, "Anthropic");
        assert!(anthropic.models.contains_key("claude-3-5-sonnet-20241022"));
    }
}
