//! Local file resolver: read model specs from a JSON file (models.dev compatible format).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use super::resolver::ModelLimitResolver;
use super::spec::ModelSpec;

/// Resolves model specs from a local JSON file.
///
/// JSON format is compatible with models.dev: `root[provider_id].models[model_id].limit`.
pub struct LocalFileResolver {
    path: PathBuf,
    data: RwLock<Option<Value>>,
}

impl LocalFileResolver {
    /// Create a new resolver for the given file path.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            data: RwLock::new(None),
        }
    }

    /// Load (or reload) JSON from disk.
    pub async fn load(&self) -> Result<(), String> {
        let contents = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| e.to_string())?;
        let json: Value = serde_json::from_str(&contents).map_err(|e| e.to_string())?;
        *self.data.write().await = Some(json);
        Ok(())
    }

    async fn ensure_loaded(&self) -> Option<Value> {
        {
            let guard = self.data.read().await;
            if guard.is_some() {
                return guard.clone();
            }
        }
        if self.load().await.is_err() {
            return None;
        }
        self.data.read().await.clone()
    }

    fn resolve_from_json(
        &self,
        json: &Value,
        provider_id: &str,
        model_id: &str,
    ) -> Option<ModelSpec> {
        let provider = json.get(provider_id)?;
        let models = provider.get("models")?.as_object()?;

        let model = models.get(model_id).or_else(|| {
            if !model_id.contains('/') {
                models.get(&format!("{}/{}", provider_id, model_id))
            } else {
                None
            }
        })?;

        super::models_dev::parse_model_limit(model)
    }
}

#[async_trait]
impl ModelLimitResolver for LocalFileResolver {
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<ModelSpec> {
        let json = self.ensure_loaded().await?;
        self.resolve_from_json(&json, provider_id, model_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn resolve_from_local_file() {
        let json = r#"{"zai":{"models":{"glm-5":{"limit":{"context":204800,"output":131072}}}}}"#;
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), json).unwrap();

        let resolver = LocalFileResolver::new(file.path());
        let spec = resolver.resolve("zai", "glm-5").await.unwrap();
        assert_eq!(spec.context_limit, 204_800);
        assert_eq!(spec.output_limit, 131_072);
    }

    #[tokio::test]
    async fn resolve_returns_none_for_missing_file() {
        let resolver = LocalFileResolver::new("/nonexistent/path/models.json");
        assert!(resolver.resolve("zai", "glm-5").await.is_none());
    }
}
