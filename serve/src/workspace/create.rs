use std::sync::Arc;

use loom::{
    ServerResponse, WorkspaceCreateRequest, WorkspaceCreateResponse,
};

use super::no_store_error;

pub(crate) async fn handle_workspace_create(
    r: WorkspaceCreateRequest,
    store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let id = r.id.clone();
    let Some(store) = store else {
        return no_store_error(&id);
    };
    match store.create_workspace(r.name).await {
        Ok(workspace_id) => {
            ServerResponse::WorkspaceCreate(WorkspaceCreateResponse { id, workspace_id })
        }
        Err(e) => ServerResponse::Error(loom::ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}
