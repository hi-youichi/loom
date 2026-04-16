mod list;
mod create;
mod rename;
mod thread_list;
mod thread_add;
mod thread_remove;

use loom::{ErrorResponse, ServerResponse};

pub(crate) use list::handle_workspace_list;
pub(crate) use create::handle_workspace_create;
pub(crate) use rename::handle_workspace_rename;
pub(crate) use thread_list::handle_workspace_thread_list;
pub(crate) use thread_add::handle_workspace_thread_add;
pub(crate) use thread_remove::handle_workspace_thread_remove;

pub(crate) fn no_store_error(id: &str) -> ServerResponse {
    ServerResponse::Error(ErrorResponse {
        id: Some(id.to_string()),
        error: "workspace store not configured (set WORKSPACE_DB)".to_string(),
    })
}
