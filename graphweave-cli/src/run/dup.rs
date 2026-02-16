//! DUP agent pattern: understand → plan → act → observe.
//!
//! Runs the DUP graph via [`build_dup_runner`](graphweave::build_dup_runner),
//! streams understand/plan/act/observe events, and returns the last assistant reply.

use graphweave::{build_dup_runner, DupState, StreamEvent};
use tracing::{info_span, Instrument};

use super::{
    build_helve_config, format_dup_state_display, print_loaded_tools, truncate_display, RunError,
    RunOptions,
};

/// Runs the DUP graph (understand → plan → act → observe).
pub async fn run_dup(opts: &RunOptions) -> Result<String, RunError> {
    let (helve, config) = build_helve_config(opts);
    let thread_id_log = config.thread_id.as_deref().unwrap_or("").to_string();
    let span = info_span!("run_dup", thread_id = %thread_id_log);
    tracing::info!(parent: &span, thread_id = %thread_id_log, "run started");

    if helve.role_setting.is_some() {
        eprintln!("SOUL.md loaded; system prompt (including it) is in state.messages[0].");
    }
    print_loaded_tools(&config).await?;

    let runner = build_dup_runner(&config, None, opts.verbose)
        .instrument(span.clone())
        .await?;

    let mut turn = 0u32;
    let mut last_node: Option<String> = None;
    let display_max_len = opts.display_max_len;

    let state: DupState = runner
        .stream_with_config(
            opts.message.as_str(),
            None,
            Some(|ev: StreamEvent<DupState>| match &ev {
                StreamEvent::TaskStart { node_id } => {
                    let from = last_node.as_deref().unwrap_or("START");
                    eprintln!("flow: {} → {}", from, node_id);
                    eprintln!("-------------------- {} --------------------", node_id);
                    last_node = Some(node_id.clone());
                }
                StreamEvent::Updates { node_id, state } => {
                    let label = match node_id.as_str() {
                        "understand" => {
                            if let Some(ref u) = state.understood {
                                eprintln!("--- Understanding ---");
                                eprintln!(
                                    "  Core goal: {}",
                                    truncate_display(&u.core_goal, display_max_len)
                                );
                                eprintln!("  Constraints: {:?}", u.key_constraints);
                                eprintln!(
                                    "  Context: {}",
                                    truncate_display(&u.relevant_context, display_max_len)
                                );
                            }
                            "state after understand".to_string()
                        }
                        "plan" => {
                            turn += 1;
                            format!("state after plan (turn {})", turn)
                        }
                        "act" => "state after act".to_string(),
                        "observe" => "state after observe".to_string(),
                        _ => format!("state after {}", node_id),
                    };
                    eprintln!("--- {} ---", label);
                    eprintln!("{}", format_dup_state_display(state, display_max_len));
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
