//! Local file resolver: reads model specs from a local JSON file.

use std::path::PathBuf;

use async_trait::async_trait;

use super::ModelResolver;
use crate::parser::parse_model;
use crate::Model;

/// Resolves models from a local JSON file in models.dev format.
///
/// File is re-read on each resolve (no caching). Wrap with `CachedResolver` for production.
pub struct LocalFileResolver {
    path: PathBuf,
}

impl LocalFileResolver {
    /// Create with the path to a local JSON file.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl ModelResolver for LocalFileResolver {
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        let body = tokio::fs::read_to_string(&self.path).await.ok()?;
        let json: serde_json::Value = serde_json::from_str(&body).ok()?;
        let provider = json.get(provider_id)?;
        let models = provider.get("models")?.as_object()?;

        let model = models.get(model_id).or_else(|| {
            if !model_id.contains('/') {
                models.get(&format!("{}/{}", provider_id, model_id))
            } else {
                None
            }
        })?;

        parse_model(model_id, model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_from_local_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("loom_local_resolver_test.json");
        let body = r#"{"zai":{"models":{"glm-5":{"limit":{"context":204800,"output":131072}}}}}"#;
        tokio::fs::write(&path, body).await.unwrap();

        let resolver = LocalFileResolver::new(path.clone());
        let model = resolver.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(model.limit.context, 204_800);
        assert_eq!(model.limit.output, 131_072);

        let _ = tokio::fs::remove_file(&path).await;
    }
}
