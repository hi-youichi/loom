//! Model request handlers.

use loom::{
    services::ModelService,
    ErrorResponse, ServerResponse,
};

/// Handle list_models request
pub(crate) async fn handle_list_models(
    request: (), // Temporarily disabled due to missing protocol definitions
    model_service: &ModelService,
) -> ServerResponse {
    let _models = model_service.get_available_models().await;
    
    ServerResponse::Error(ErrorResponse {
        id: None,
        error: "ListModels not implemented".to_string(),
    })
}

/// Handle set_model request  
pub(crate) async fn handle_set_model(
    request: (), // Temporarily disabled due to missing protocol definitions
    model_service: &ModelService,
) -> ServerResponse {
    let _ = model_service; // Use the parameter to avoid warning
    
    ServerResponse::Error(ErrorResponse {
        id: None,
        error: "SetModel not implemented".to_string(),
    })
}
