//! Model request handlers.

use loom_tier::{ModelRegistry, ProviderConfig};
use loom_protocol::responses::ModelInfo;
use loom_protocol::ServerResponse;

/// Handle list_models request
pub(crate) async fn handle_list_models(
    request: loom_protocol::requests::ListModelsRequest,
    providers: &[ProviderConfig],
) -> ServerResponse {
    let registry = ModelRegistry::global();
    let model_entries = registry.list_all_models(providers).await;

    tracing::info!(
        "📋 Listing {} available models for frontend",
        model_entries.len()
    );

    let models = model_entries
        .into_iter()
        .map(|entry| {
            tracing::debug!(
                "🤖 Model: id={}, name={}, provider={}",
                entry.id,
                entry.name,
                entry.provider
            );
            ModelInfo {
                id: entry.id.clone(),
                name: entry.name.clone(),
                provider: entry.provider.clone(),
                family: None,       // ModelEntry doesn't provide family info
                capabilities: None, // ModelEntry doesn't provide capabilities info
            }
        })
        .collect();

    ServerResponse::ListModels(loom_protocol::responses::ListModelsResponse {
        id: request.id,
        models,
    })
}

/// Handle set_model request  
pub(crate) async fn handle_set_model(
    request: loom_protocol::requests::SetModelRequest,
    _providers: &[ProviderConfig],
) -> ServerResponse {
    // For now, session model setting is not implemented in the backend
    // Return success but indicate the limitation
    ServerResponse::SetModel(loom_protocol::responses::SetModelResponse {
        id: request.id,
        success: false,
        error: Some("Session model setting not yet implemented".to_string()),
    })
}
