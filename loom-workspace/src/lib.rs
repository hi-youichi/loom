//! Workspace and thread association: separate crate with its own SQLite storage.
//!
//! - **Workspace**: container for threads (1 workspace : N threads).
//! - **Run with workspace_id**: when serve handles a Run request with both `workspace_id` and
//!   `thread_id`, it registers the thread in that workspace.
//! - **UI**: use `list_threads(workspace_id)` to show "某 workspace 下所有对话列表".

mod store;

pub use store::{Store, StoreError, ThreadInWorkspace, WorkspaceMeta};
