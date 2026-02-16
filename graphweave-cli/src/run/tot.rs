//! ToT (Tree of Thought) agent pattern: think_expand → think_evaluate → act → observe.
//!
//! Runs the ToT graph via [`build_tot_runner`](graphweave::build_tot_runner),
//! streams expand/evaluate/backtrack events, and returns the last assistant reply.

use graphweave::{build_tot_runner, StreamEvent, TotState};
use tracing::{info_span, Instrument};

use super::{
    build_helve_config, format_tot_state_display, print_loaded_tools, RunError, RunOptions,
};

/// Runs the ToT graph (think_expand → think_evaluate → act → observe).
pub async fn run_tot(opts: &RunOptions) -> Result<String, RunError> {
    let (helve, config) = build_helve_config(opts);
    let thread_id_log = config.thread_id.as_deref().unwrap_or("").to_string();
    let span = info_span!("run_tot", thread_id = %thread_id_log);
    tracing::info!(parent: &span, thread_id = %thread_id_log, "run started");

    if helve.role_setting.is_some() {
        eprintln!("SOUL.md loaded; system prompt (including it) is in state.messages[0].");
    }
    print_loaded_tools(&config).await?;

    let runner = build_tot_runner(&config, None, opts.verbose)
        .instrument(span.clone())
        .await?;

    let mut last_node: Option<String> = None;
    let display_max_len = opts.display_max_len;

    let state: TotState = runner
        .stream_with_config(
            opts.message.as_str(),
            None,
            Some(|ev: StreamEvent<TotState>| match &ev {
                StreamEvent::TaskStart { node_id } => {
                    let from = last_node.as_deref().unwrap_or("START");
                    eprintln!("flow: {} → {}", from, node_id);
                    eprintln!("-------------------- {} --------------------", node_id);
                    last_node = Some(node_id.clone());
                }
                StreamEvent::TotExpand { candidates } => {
                    eprintln!("--- ToT expand: {} candidates ---", candidates.len());
                    for (i, c) in candidates.iter().enumerate() {
                        eprintln!("  [{}] {}", i + 1, c);
                    }
                }
                StreamEvent::TotEvaluate { chosen, scores } => {
                    eprintln!(
                        "--- ToT evaluate: chosen={}, scores={:?} ---",
                        chosen, scores
                    );
                }
                StreamEvent::TotBacktrack { reason, to_depth } => {
                    eprintln!(
                        "--- ToT backtrack: reason={}, to_depth={} ---",
                        reason, to_depth
                    );
                }
                StreamEvent::Updates { node_id, state } => {
                    let label = match node_id.as_str() {
                        "think_expand" => "state after think_expand".to_string(),
                        "think_evaluate" => "state after think_evaluate".to_string(),
                        "act" => "state after act".to_string(),
                        "observe" => "state after observe".to_string(),
                        _ => format!("state after {}", node_id),
                    };
                    eprintln!("--- {} ---", label);
                    eprintln!("{}", format_tot_state_display(state, display_max_len));
                }
                _ => {}
            }),
        )
        .instrument(span)
        .await?;

    if let Some(from) = last_node.as_deref() {
        eprintln!("flow: {} → END", from);
    }

    let reply = state.last_assistant_reply().unwrap_or_default();
    Ok(reply)
}
