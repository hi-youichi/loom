//! Model service for managing available models and model metadata.

use crate::protocol::responses::ModelInfo;
use model_spec_core::spec::{Model, Provider};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Model service for managing available models
#[derive(Clone)]
pub struct ModelService {
    providers: Arc<RwLock<HashMap<String, Provider>>>,
    models: Arc<RwLock<HashMap<String, Model>>>,
}

impl ModelService {
    /// Create a new model service
    pub fn new() -> Self {
        
        
        Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
            models: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load providers and models from models.dev
    pub async fn load_from_models_dev(&self) -> Result<(), String> {
        let url = "https://models.dev/api.json";
        tracing::info!("Fetching models from {}", url);
        
        let response = reqwest::get(url)
            .await
            .map_err(|e| {
                tracing::error!("Failed to fetch from models.dev: {}", e);
                format!("Failed to fetch from models.dev: {}", e)
            })?;
        
        let status = response.status();
        tracing::info!("Response status: {}", status);
        
        if !status.is_success() {
            return Err(format!("Models.dev returned status: {}", status));
        }
        
        let providers_json: HashMap<String, Provider> = response
            .json()
            .await
            .map_err(|e| {
                tracing::error!("Failed to parse models.dev response: {}", e);
                format!("Failed to parse models.dev response: {}", e)
            })?;

        let mut providers = self.providers.write().await;
        let mut models = self.models.write().await;
        
        let mut total_models = 0;
        for (provider_id, provider) in providers_json {
            for (model_id, model) in &provider.models {
                models.insert(model_id.clone(), model.clone());
                total_models += 1;
            }
            providers.insert(provider_id, provider);
        }
        
        tracing::info!("Loaded {} models from models.dev", total_models);
        Ok(())
    }

    /// Get all available models
    pub async fn get_available_models(&self) -> Vec<ModelInfo> {
        let models = self.models.read().await;
        let providers = self.providers.read().await;
        
        models.values()
            .map(|model| {
                let provider = model.id.split('-').next()
                    .and_then(|provider_id| providers.get(provider_id))
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                
                ModelInfo {
                    id: model.id.clone(),
                    name: model.name.clone(),
                    provider,
                    family: model.family.clone(),
                    capabilities: Self::extract_capabilities(model),
                }
            })
            .collect()
    }

    /// Get model by ID
    pub async fn get_model(&self, model_id: &str) -> Option<Model> {
        let models = self.models.read().await;
        models.get(model_id).cloned()
    }

    /// Extract capabilities from model metadata
    fn extract_capabilities(model: &Model) -> Option<Vec<String>> {
        let mut capabilities = Vec::new();
        
        if model.tool_call {
            capabilities.push("tool_call".to_string());
        }
        if model.attachment {
            capabilities.push("attachment".to_string());
        }
        if model.reasoning {
            capabilities.push("reasoning".to_string());
        }
        if model.structured_output.unwrap_or(false) {
            capabilities.push("structured_output".to_string());
        }
        
        if capabilities.is_empty() {
            None
        } else {
            Some(capabilities)
        }
    }

    /// Validate model availability
    pub async fn is_model_available(&self, model_id: &str) -> bool {
        let models = self.models.read().await;
        models.contains_key(model_id)
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
    fn test_model_service_creation() {
        let service = ModelService::new();
        // Service should be created successfully
        let models = tokio::runtime::Runtime::new().unwrap().block_on(service.get_available_models());
        assert!(models.is_empty(), "Model service should start with no models");
    }

    #[test]
    fn test_model_service_default() {
        let service = ModelService::default();
        let models = tokio::runtime::Runtime::new().unwrap().block_on(service.get_available_models());
        assert!(models.is_empty(), "Default model service should start with no models");
    }
}
