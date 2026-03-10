//! Spawns a loom agent run (used by webhook handler when no test callback is set).

use loom::{run_agent_with_options, RunCmd, RunOptions};

/// Spawns an async task that runs the loom agent with the given options.
/// Returns immediately; the task runs in the background. Logs thread_id for
/// "spawning", "finished", and "failed".
pub fn spawn_agent_run(opts: RunOptions) {
    let opts = opts.clone();
    let thread_id = opts.thread_id.clone();
    tokio::spawn(async move {
        tracing::info!(thread_id = ?thread_id, "spawning loom agent run");
        match run_agent_with_options(&opts, &RunCmd::React, None).await {
            Ok(_reply) => {
                tracing::info!(thread_id = ?thread_id, "loom agent run finished");
            }
            Err(e) => {
                tracing::error!(thread_id = ?thread_id, error = %e, "loom agent run failed");
            }
        }
    });
}
