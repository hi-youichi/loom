use std::sync::Arc;

use loom::{
    ServerResponse, WorkspaceListRequest, WorkspaceListResponse, WorkspaceMeta,
};

use super::no_store_error;

pub(crate) async fn handle_workspace_list(
    r: WorkspaceListRequest,
    store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let id = r.id.clone();
    let Some(store) = store else {
        return no_store_error(&id);
    };
    match store.list_workspaces().await {
        Ok(workspaces) => {
            let workspaces = workspaces
                .into_iter()
                .map(|w| WorkspaceMeta {
                    id: w.id,
                    name: w.name,
                    created_at_ms: w.created_at_ms,
                })
                .collect();
            ServerResponse::WorkspaceList(WorkspaceListResponse { id, workspaces })
        }
        Err(e) => ServerResponse::Error(loom::ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}
