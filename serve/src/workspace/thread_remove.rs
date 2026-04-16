use std::sync::Arc;

use loom::{
    ServerResponse, WorkspaceThreadRemoveRequest, WorkspaceThreadRemoveResponse,
};

use super::no_store_error;

pub(crate) async fn handle_workspace_thread_remove(
    r: WorkspaceThreadRemoveRequest,
    store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let id = r.id.clone();
    let workspace_id = r.workspace_id.clone();
    let thread_id = r.thread_id.clone();
    let Some(store) = store else {
        return no_store_error(&id);
    };
    match store
        .remove_thread_from_workspace(&r.workspace_id, &r.thread_id)
        .await
    {
        Ok(()) => ServerResponse::WorkspaceThreadRemove(WorkspaceThreadRemoveResponse {
            id,
            workspace_id,
            thread_id,
        }),
        Err(e) => ServerResponse::Error(loom::ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}
