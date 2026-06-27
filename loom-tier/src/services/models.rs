//! Model service for managing available models and model metadata.

use model_spec_core::{Model, Provider};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Model information for API responses.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
}

/// Model service for managing available models
#[derive(Clone)]
pub struct ModelService {
    providers: Arc<RwLock<HashMap<String, Provider>>>,
    models: Arc<RwLock<HashMap<String, Model>>>,
    model_to_provider: Arc<RwLock<HashMap<String, String>>>,
}

impl ModelService {
    /// Create a new model service
    pub fn new() -> Self {
        Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
            models: Arc::new(RwLock::new(HashMap::new())),
            model_to_provider: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load providers and models from models.dev
    pub async fn load_from_models_dev(&self) -> Result<(), String> {
        let url = "https://models.dev/api.json";
        tracing::info!("Fetching models from {}", url);

        let response = reqwest::get(url).await.map_err(|e| {
            tracing::error!("Failed to fetch from models.dev: {}", e);
            format!("Failed to fetch from models.dev: {}", e)
        })?;

        let status = response.status();
        tracing::info!("Response status: {}", status);

        if !status.is_success() {
            return Err(format!("Models.dev returned status: {}", status));
        }

        let json_value: serde_json::Value = response.json().await.map_err(|e| {
            tracing::error!("Failed to read response body: {}", e);
            format!("Failed to read response body: {}", e)
        })?;

        tracing::debug!("Response structure: {:?}", json_value);

        if let Some(providers_array) = json_value.as_array() {
            self.parse_providers_array(providers_array).await?;
        } else if let Some(providers_obj) = json_value.as_object() {
            self.parse_providers_object(providers_obj).await?;
        } else {
            return Err("Unknown JSON structure from models.dev".to_string());
        }

        Ok(())
    }

    async fn parse_providers_array(&self, providers_array: &[serde_json::Value]) -> Result<(), String> {
        tracing::info!("Parsing {} providers from array", providers_array.len());

        let mut providers_guard = self.providers.write().await;
        let mut models_guard = self.models.write().await;
        let mut model_to_provider_guard = self.model_to_provider.write().await;

        for provider_value in providers_array {
            if let Ok(provider) = serde_json::from_value::<Provider>(provider_value.clone()) {
                tracing::debug!("Loaded provider: {}", provider.id);

                providers_guard.insert(provider.id.clone(), provider.clone());

                for (model_id, model) in &provider.models {
                    tracing::debug!("Loaded model: {} from provider {}", model_id, provider.id);
                    models_guard.insert(model_id.clone(), model.clone());
                    model_to_provider_guard.insert(model_id.clone(), provider.id.clone());
                }
            }
        }

        tracing::info!("Loaded {} providers and {} models total",
                     providers_guard.len(), models_guard.len());

        Ok(())
    }

    async fn parse_providers_object(&self, providers_obj: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
        tracing::info!("Parsing providers from object with {} keys", providers_obj.len());

        let mut providers_guard = self.providers.write().await;
        let mut models_guard = self.models.write().await;
        let mut model_to_provider_guard = self.model_to_provider.write().await;

        for (provider_id, provider_value) in providers_obj {
            if let Some(models_array) = provider_value.as_array() {
                tracing::debug!("Processing provider {} with {} models", provider_id, models_array.len());

                let provider = Provider {
                    id: provider_id.clone(),
                    name: provider_id.clone(),
                    api: None,
                    models: HashMap::new(),
                    doc: None,
                    env: vec![],
                    npm: None,
                };

                for model_value in models_array {
                    if let Ok(model) = serde_json::from_value::<Model>(model_value.clone()) {
                        tracing::debug!("Loaded model: {} from provider {}", model.id, provider_id);
                        models_guard.insert(model.id.clone(), model.clone());
                        model_to_provider_guard.insert(model.id.clone(), provider.id.clone());
                    }
                }

                providers_guard.insert(provider.id.clone(), provider);
            }
        }

        tracing::info!("Loaded {} providers and {} models total",
                     providers_guard.len(), models_guard.len());

        Ok(())
    }

    /// Get all available providers
    pub async fn get_providers(&self) -> HashMap<String, Provider> {
        self.providers.read().await.clone()
    }

    /// Get all available models
    pub async fn get_models(&self) -> HashMap<String, Model> {
        self.models.read().await.clone()
    }

    /// Get provider for a specific model
    pub async fn get_provider_for_model(&self, model_id: &str) -> Option<String> {
        self.model_to_provider.read().await.get(model_id).cloned()
    }

    /// Get specific model by ID
    pub async fn get_model(&self, model_id: &str) -> Option<Model> {
        self.models.read().await.get(model_id).cloned()
    }

    /// Get specific provider by ID
    pub async fn get_provider(&self, provider_id: &str) -> Option<Provider> {
        self.providers.read().await.get(provider_id).cloned()
    }

    /// Get models filtered by provider
    pub async fn get_models_by_provider(&self, provider_id: &str) -> Vec<Model> {
        let models = self.models.read().await;
        let provider_map = self.model_to_provider.read().await;

        models.iter()
            .filter(|(id, _)| provider_map.get(*id).is_some_and(|p| p == provider_id))
            .map(|(_, model)| model.clone())
            .collect()
    }

    /// Convert models to ModelInfo format for API responses
    pub async fn to_model_info_list(&self) -> Vec<ModelInfo> {
        let models = self.models.read().await;
        let provider_map = self.model_to_provider.read().await;

        models.iter()
            .map(|(id, model)| {
                let provider_id = provider_map.get(id).cloned().unwrap_or_default();
                ModelInfo {
                    id: id.clone(),
                    name: model.name.clone(),
                    provider: provider_id,
                    family: model.family.clone(),
                    capabilities: None,
                }
            })
            .collect()
    }
}

impl Default for ModelService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_service_new() {
        let service = ModelService::new();
        let providers = tokio::runtime::Runtime::new().unwrap().block_on(async {
            service.get_providers().await
        });
        assert!(providers.is_empty());
    }

    #[test]
    fn test_model_service_default() {
        let service = ModelService::default();
        let providers = tokio::runtime::Runtime::new().unwrap().block_on(async {
            service.get_providers().await
        });
        assert!(providers.is_empty());
    }

    #[test]
    fn test_model_service_empty_state() {
        let service = ModelService::new();
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            assert!(service.get_providers().await.is_empty());
            assert!(service.get_models().await.is_empty());
            assert!(service.get_provider_for_model("test").await.is_none());
            assert!(service.get_model("test").await.is_none());
            assert!(service.get_provider("test").await.is_none());
        });
    }

    #[test]
    fn test_model_service_cloning() {
        let service = ModelService::new();
        let cloned = service.clone();
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let providers1 = service.get_providers().await;
            let providers2 = cloned.get_providers().await;
            assert_eq!(providers1.len(), providers2.len());
        });
    }

    #[test]
    fn test_model_info_conversion_empty() {
        let service = ModelService::new();
        let rt = tokio::runtime::Runtime::new().unwrap();

        let result = rt.block_on(async {
            service.to_model_info_list().await
        });

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_get_models_by_provider_empty() {
        let service = ModelService::new();
        let result = service.get_models_by_provider("test_provider").await;
        assert!(result.is_empty());
    }

    #[test]
    fn test_arc_rwlock_structure() {
        let service = ModelService::new();
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let providers = service.providers.read().await;
            let models = service.models.read().await;
            let model_to_provider = service.model_to_provider.read().await;

            assert!(providers.is_empty());
            assert!(models.is_empty());
            assert!(model_to_provider.is_empty());
        });
    }
}