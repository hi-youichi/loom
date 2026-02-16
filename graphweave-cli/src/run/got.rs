//! GoT (Graph of Thoughts) agent pattern: plan_graph → execute_graph.
//!
//! Runs the GoT graph via [`build_got_runner`](graphweave::build_got_runner),
//! streams plan/node events, and returns the summary result.
//! Supports AGoT adaptive mode when `RunOptions::got_adaptive` is true.

use graphweave::{build_got_runner, GotState, StreamEvent};
use tracing::{info_span, Instrument};

use super::{
    build_helve_config, format_got_state_display, print_loaded_tools, RunError, RunOptions,
};

/// Runs the GoT graph (plan_graph → execute_graph).
pub async fn run_got(opts: &RunOptions) -> Result<String, RunError> {
    let (helve, mut config) = build_helve_config(opts);
    config.got_adaptive = opts.got_adaptive;

    let thread_id_log = config.thread_id.as_deref().unwrap_or("").to_string();
    let span = info_span!("run_got", thread_id = %thread_id_log);
    tracing::info!(parent: &span, thread_id = %thread_id_log, "run started");

    if helve.role_setting.is_some() {
        eprintln!("SOUL.md loaded; system prompt (including it) is in state.messages[0].");
    }
    print_loaded_tools(&config).await?;

    let runner = build_got_runner(&config, None, opts.verbose)
        .instrument(span.clone())
        .await?;

    let mut last_node: Option<String> = None;
    let display_max_len = opts.display_max_len;

    let state: GotState = runner
        .stream_with_config(
            opts.message.as_str(),
            None,
            Some(|ev: StreamEvent<GotState>| match &ev {
                StreamEvent::TaskStart { node_id } => {
                    let from = last_node.as_deref().unwrap_or("START");
                    eprintln!("flow: {} → {}", from, node_id);
                    eprintln!("-------------------- {} --------------------", node_id);
                    last_node = Some(node_id.clone());
                }
                StreamEvent::GotPlan {
                    node_count,
                    edge_count,
                    node_ids,
                } => {
                    eprintln!(
                        "--- GoT plan: {} nodes, {} edges ---",
                        node_count, edge_count
                    );
                    for id in node_ids {
                        eprintln!("  node: {}", id);
                    }
                }
                StreamEvent::GotNodeStart { node_id } => {
                    eprintln!("--- GoT node start: {} ---", node_id);
                }
                StreamEvent::GotNodeComplete {
                    node_id,
                    result_summary,
                } => {
                    eprintln!("--- GoT node complete: {} ---", node_id);
                    eprintln!("  result: {}", result_summary);
                }
                StreamEvent::GotNodeFailed { node_id, error } => {
                    eprintln!("--- GoT node failed: {} ---", node_id);
                    eprintln!("  error: {}", error);
                }
                StreamEvent::GotExpand {
                    node_id,
                    nodes_added,
                    edges_added,
                } => {
                    eprintln!(
                        "--- AGoT expand: {} → +{} nodes, +{} edges ---",
                        node_id, nodes_added, edges_added
                    );
                }
                StreamEvent::Updates { node_id, state } => {
                    eprintln!("--- state after {} ---", node_id);
                    eprintln!("{}", format_got_state_display(state, display_max_len));
                }
                _ => {}
            }),
        )
        .instrument(span)
        .await?;

    if let Some(from) = last_node.as_deref() {
        eprintln!("flow: {} → END", from);
    }

    eprintln!("--- GoT final state ---");
    eprintln!("{}", format_got_state_display(&state, display_max_len));

    let reply = state.summary_result();
    Ok(reply)
}
