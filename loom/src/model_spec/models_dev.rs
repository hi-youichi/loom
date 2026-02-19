//! Models.dev resolver: fetch model specs from https://models.dev/api.json

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::resolver::ModelLimitResolver;
use super::spec::ModelSpec;

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
        let body = client
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        Ok(body)
    }
}

/// Resolves model specs from models.dev API.
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
    pub async fn fetch_all(&self) -> Result<std::collections::HashMap<String, ModelSpec>, String> {
        let body = self.http_client.get(&self.base_url).await?;
        parse_all_models(&body)
    }

    fn resolve_from_json(&self, json: &Value, provider_id: &str, model_id: &str) -> Option<ModelSpec> {
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

        parse_model_limit(model)
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

/// Parse ModelSpec from a model JSON object (has "limit" with "context" and "output").
pub(crate) fn parse_model_limit(model: &Value) -> Option<ModelSpec> {
    let limit = model.get("limit")?;
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

    let mut spec = ModelSpec::new(context, output);
    if let Some(v) = cache_read {
        spec.cache_read = Some(v);
    }
    if let Some(v) = cache_write {
        spec.cache_write = Some(v);
    }
    Some(spec)
}

fn parse_all_models(body: &str) -> Result<std::collections::HashMap<String, ModelSpec>, String> {
    let json: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut out = std::collections::HashMap::new();

    let providers = json.as_object().ok_or("root is not an object")?;
    for (provider_id, provider) in providers {
        let models = match provider.get("models").and_then(|m| m.as_object()) {
            Some(m) => m,
            None => continue,
        };
        for (model_id, model) in models {
            if let Some(spec) = parse_model_limit(model) {
                let key = format!("{}/{}", provider_id, model_id);
                out.insert(key, spec);
            }
        }
    }
    Ok(out)
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
            "zenmux": {
                "models": {
                    "openai/gpt-5": {
                        "limit": { "context": 400000, "output": 64000 }
                    },
                    "anthropic/claude-sonnet-4": {
                        "limit": { "context": 1000000, "output": 64000 }
                    }
                }
            },
            "zai": {
                "models": {
                    "glm-5": {
                        "limit": { "context": 204800, "output": 131072 }
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
        let resolver = ModelsDevResolver::with_client(
            "https://example.com/api.json".to_string(),
            client,
        );

        let spec = resolver.resolve("zenmux", "openai/gpt-5").await.unwrap();
        assert_eq!(spec.context_limit, 400_000);
        assert_eq!(spec.output_limit, 64_000);

        let spec = resolver.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(spec.context_limit, 204_800);
        assert_eq!(spec.output_limit, 131_072);
    }

    #[tokio::test]
    async fn resolve_returns_none_for_unknown_model() {
        let client = Arc::new(MockHttpClient {
            body: fixture_json(),
        });
        let resolver = ModelsDevResolver::with_client(
            "https://example.com/api.json".to_string(),
            client,
        );

        assert!(resolver.resolve("zenmux", "unknown-model").await.is_none());
        assert!(resolver.resolve("unknown-provider", "gpt-5").await.is_none());
    }

    #[tokio::test]
    async fn fetch_all_parses_all_providers() {
        let client = Arc::new(MockHttpClient {
            body: fixture_json(),
        });
        let resolver = ModelsDevResolver::with_client(
            "https://example.com/api.json".to_string(),
            client,
        );

        let all = resolver.fetch_all().await.unwrap();
        assert!(all.contains_key("zenmux/openai/gpt-5"));
        assert!(all.contains_key("zenmux/anthropic/claude-sonnet-4"));
        assert!(all.contains_key("zai/glm-5"));
    }
}
