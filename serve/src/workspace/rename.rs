use std::sync::Arc;

use loom::{
    ServerResponse, WorkspaceRenameRequest, WorkspaceRenameResponse,
};

use super::no_store_error;

pub(crate) async fn handle_workspace_rename(
    r: WorkspaceRenameRequest,
    store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let id = r.id.clone();
    let workspace_id = r.workspace_id.clone();
    let name = r.name.clone();
    let Some(store) = store else {
        return no_store_error(&id);
    };
    match store.rename_workspace(&r.workspace_id, &r.name).await {
        Ok(()) => ServerResponse::WorkspaceRename(WorkspaceRenameResponse {
            id,
            workspace_id,
            name,
        }),
        Err(e) => ServerResponse::Error(loom::ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}
