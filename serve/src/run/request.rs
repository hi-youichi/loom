//! Request preparation: register thread in workspace, append initial user message, build RunOptions and RunCmd.

use loom::{AgentType, Message, RunCmd, RunOptions};
use std::path::PathBuf;
use std::sync::Arc;

/// Registers the run's thread in the given workspace when all of workspace_id, thread_id,
/// and workspace_store are present (run-time association for UI: "thread belongs to workspace").
/// Missing any of the three is a no-op. On store error only logs a warning and does not
/// fail the run.
pub(super) async fn try_register_thread_in_workspace(
    workspace_store: Option<&Arc<loom_workspace::Store>>,
    workspace_id: Option<&str>,
    thread_id: Option<&str>,
) {
    let Some(store) = workspace_store else { return };
    let Some(ws_id) = workspace_id else { return };
    let Some(thread_id) = thread_id else { return };
    if let Err(e) = store.add_thread_to_workspace(ws_id, thread_id).await {
        tracing::warn!("workspace add_thread_to_workspace: {}", e);
    }
}

/// Appends the initial user message to the per-thread message store when both thread_id
/// and user_message_store are set. Returns `true` if append was performed (caller may use
/// this to set initial message count for the run). Returns `false` if store or thread_id
/// is missing, or append I/O failed (only a warning is logged; run is not failed).
pub(super) async fn try_append_initial_user_message(
    user_message_store: Option<&Arc<dyn loom::UserMessageStore>>,
    thread_id: Option<&str>,
    message: &str,
) -> bool {
    let Some(store) = user_message_store else { return false };
    let Some(thread_id) = thread_id else { return false };
    let msg = Message::user(message);
    match store.append(thread_id, &msg).await {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("user_message_store append initial user: {}", e);
            false
        }
    }
}

/// Input for building run options and command from a Run request.
pub(super) struct PrepareRunInput {
    pub display_max_len: usize,
}

/// Result of request preparation: options, command, and whether the initial user message was appended.
pub(super) struct PrepareRunResult {
    pub opts: RunOptions,
    pub cmd: RunCmd,
    pub initial_user_appended: bool,
}

/// Registers thread in workspace, appends initial user message when configured, and builds
/// RunOptions and RunCmd from the request. Used by [`crate::run::handle_run`].
pub(super) async fn prepare_run(
    r: loom::RunRequest,
    workspace_store: Option<&Arc<loom_workspace::Store>>,
    user_message_store: Option<&Arc<dyn loom::UserMessageStore>>,
    input: PrepareRunInput,
) -> PrepareRunResult {
    try_register_thread_in_workspace(
        workspace_store,
        r.workspace_id.as_deref(),
        r.thread_id.as_deref(),
    )
    .await;

    let initial_user_appended = try_append_initial_user_message(
        user_message_store,
        r.thread_id.as_deref(),
        r.message.as_str(),
    )
    .await;

    let opts = RunOptions {
        message: r.message,
        working_folder: r.working_folder.map(PathBuf::from),
        thread_id: r.thread_id,
        role_file: None,
        verbose: r.verbose.unwrap_or(false),
        got_adaptive: r.got_adaptive.unwrap_or(false),
        display_max_len: input.display_max_len,
        output_json: true,
    };
    let cmd = match r.agent {
        AgentType::React => RunCmd::React,
        AgentType::Dup => RunCmd::Dup,
        AgentType::Tot => RunCmd::Tot,
        AgentType::Got => RunCmd::Got {
            got_adaptive: opts.got_adaptive,
        },
    };

    PrepareRunResult {
        opts,
        cmd,
        initial_user_appended,
    }
}
