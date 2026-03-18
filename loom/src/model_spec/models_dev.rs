//! Models.dev resolver: fetch model specs from https://models.dev/api.json

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::http_retry::{
    is_retryable_reqwest_error, retry_backoff_for_attempt, TRANSIENT_HTTP_MAX_RETRIES,
};

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
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);

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
        assert!(all.contains_key("zenmux/openai/gpt-5"));
        assert!(all.contains_key("zenmux/anthropic/claude-sonnet-4"));
        assert!(all.contains_key("zai/glm-5"));
    }

    #[test]
    fn new_uses_default_url_and_reqwest_client() {
        let resolver = ModelsDevResolver::new();
        assert_eq!(resolver.base_url, DEFAULT_MODELS_DEV_URL);
    }

    #[test]
    fn default_creates_same_as_new() {
        let resolver = ModelsDevResolver::default();
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
        // Provider "zai" has model key "zai/glm-5" (provider-prefixed); lookup with model_id "glm-5" uses fallback
        let json = r#"{
            "zai": {
                "models": {
                    "zai/glm-5": {
                        "limit": { "context": 100000, "output": 50000 }
                    }
                }
            }
        }"#;
        let client = Arc::new(MockHttpClient {
            body: json.to_string(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        let spec = resolver.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(spec.context_limit, 100_000);
        assert_eq!(spec.output_limit, 50_000);
    }

    #[tokio::test]
    async fn fetch_all_with_cache_read_and_cache_write() {
        let json = r#"{
            "provider": {
                "models": {
                    "model-with-cache": {
                        "limit": {
                            "context": 128000,
                            "output": 16000,
                            "cache_read": 100000,
                            "cache_write": 8000
                        }
                    }
                }
            }
        }"#;
        let client = Arc::new(MockHttpClient {
            body: json.to_string(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        let all = resolver.fetch_all().await.unwrap();
        let spec = all.get("provider/model-with-cache").unwrap();
        assert_eq!(spec.context_limit, 128_000);
        assert_eq!(spec.output_limit, 16_000);
        assert_eq!(spec.cache_read, Some(100_000));
        assert_eq!(spec.cache_write, Some(8_000));
    }

    #[tokio::test]
    async fn fetch_all_fails_on_invalid_json() {
        let client = Arc::new(MockHttpClient {
            body: "invalid json {{{".to_string(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        let result = resolver.fetch_all().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_all_fails_when_root_is_not_object() {
        let client = Arc::new(MockHttpClient {
            body: "[1, 2, 3]".to_string(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        let result = resolver.fetch_all().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not an object"));
    }

    #[tokio::test]
    async fn fetch_all_skips_providers_without_models() {
        let json = r#"{
            "valid": {
                "models": {
                    "m1": { "limit": { "context": 1000, "output": 500 } }
                }
            },
            "no_models": {},
            "models_not_object": {
                "models": "not an object"
            }
        }"#;
        let client = Arc::new(MockHttpClient {
            body: json.to_string(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        let all = resolver.fetch_all().await.unwrap();
        assert!(all.contains_key("valid/m1"));
        assert!(!all.contains_key("no_models/anything"));
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn fetch_all_skips_models_with_invalid_limit() {
        let json = r#"{
            "p": {
                "models": {
                    "valid": { "limit": { "context": 1000, "output": 500 } },
                    "no_limit": {},
                    "bad_context": { "limit": { "context": "not number", "output": 500 } }
                }
            }
        }"#;
        let client = Arc::new(MockHttpClient {
            body: json.to_string(),
        });
        let resolver =
            ModelsDevResolver::with_client("https://example.com/api.json".to_string(), client);
        let all = resolver.fetch_all().await.unwrap();
        assert!(all.contains_key("p/valid"));
        assert!(!all.contains_key("p/no_limit"));
        assert!(!all.contains_key("p/bad_context"));
        assert_eq!(all.len(), 1);
    }
}
