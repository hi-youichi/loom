//! Handle workspace-related requests.

use std::sync::Arc;

use loom::{
    ErrorResponse, ServerResponse, WorkspaceCreateRequest, WorkspaceCreateResponse,
    WorkspaceListRequest, WorkspaceListResponse, WorkspaceMeta,
    WorkspaceThreadAddRequest, WorkspaceThreadAddResponse,
    WorkspaceThreadListRequest, WorkspaceThreadListResponse,
    WorkspaceThreadRemoveRequest, WorkspaceThreadRemoveResponse,
    ThreadInWorkspace,
};

fn no_store_error(id: &str) -> ServerResponse {
    ServerResponse::Error(ErrorResponse {
        id: Some(id.to_string()),
        error: "workspace store not configured (set WORKSPACE_DB)".to_string(),
    })
}

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
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}

pub(crate) async fn handle_workspace_create(
    r: WorkspaceCreateRequest,
    store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let id = r.id.clone();
    let Some(store) = store else {
        return no_store_error(&id);
    };
    match store.create_workspace(r.name).await {
        Ok(workspace_id) => ServerResponse::WorkspaceCreate(WorkspaceCreateResponse {
            id,
            workspace_id,
        }),
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}

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
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}

pub(crate) async fn handle_workspace_thread_add(
    r: WorkspaceThreadAddRequest,
    store: Option<Arc<loom_workspace::Store>>,
) -> ServerResponse {
    let id = r.id.clone();
    let workspace_id = r.workspace_id.clone();
    let thread_id = r.thread_id.clone();
    let Some(store) = store else {
        return no_store_error(&id);
    };
    match store.add_thread_to_workspace(&r.workspace_id, &r.thread_id).await {
        Ok(()) => ServerResponse::WorkspaceThreadAdd(WorkspaceThreadAddResponse {
            id,
            workspace_id,
            thread_id,
        }),
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}

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
    match store.remove_thread_from_workspace(&r.workspace_id, &r.thread_id).await {
        Ok(()) => ServerResponse::WorkspaceThreadRemove(WorkspaceThreadRemoveResponse {
            id,
            workspace_id,
            thread_id,
        }),
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}
