use std::sync::Arc;

use loom::{
    ServerResponse, ThreadInWorkspace, WorkspaceThreadListRequest,
    WorkspaceThreadListResponse,
};

use super::no_store_error;

pub(crate) async fn handle_workspace_thread_list(
    r: WorkspaceThreadListRequest,
    store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let id = r.id.clone();
    let workspace_id = r.workspace_id.clone();
    let Some(store) = store else {
        return no_store_error(&id);
    };
    match store.list_threads(&r.workspace_id).await {
        Ok(threads) => {
            let threads = threads
                .into_iter()
                .map(|t| ThreadInWorkspace {
                    thread_id: t.thread_id,
                    created_at_ms: t.created_at_ms,
                })
                .collect();
            ServerResponse::WorkspaceThreadList(WorkspaceThreadListResponse {
                id,
                workspace_id,
                threads,
            })
        }
        Err(e) => ServerResponse::Error(loom::ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}
