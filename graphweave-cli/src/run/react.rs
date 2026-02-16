//! ReAct agent pattern: think → act → observe.
//!
//! Runs the ReAct graph via [`build_react_runner`](graphweave::build_react_runner),
//! streams events, and returns the last assistant reply.

use graphweave::{build_react_runner, ReActState, StreamEvent};
use tracing::{info_span, Instrument};

use super::{
    build_helve_config, format_react_state_display, print_loaded_tools, RunError, RunOptions,
};

/// Runs the ReAct graph (think → act → observe).
pub async fn run_react(opts: &RunOptions) -> Result<String, RunError> {
    let (helve, config) = build_helve_config(opts);
    let thread_id_log = config.thread_id.as_deref().unwrap_or("").to_string();
    let span = info_span!("run", thread_id = %thread_id_log);
    tracing::info!(parent: &span, thread_id = %thread_id_log, "run started");

    if helve.role_setting.is_some() {
        eprintln!("SOUL.md loaded; system prompt (including it) is in state.messages[0].");
    }
    print_loaded_tools(&config).await?;

    let runner = build_react_runner(&config, None, opts.verbose, None)
        .instrument(span.clone())
        .await?;

    let mut turn = 0u32;
    let mut last_node: Option<String> = None;
    let display_max_len = opts.display_max_len;

    let state: ReActState = runner
        .stream_with_config(
            opts.message.as_str(),
            None,
            Some(|ev: StreamEvent<ReActState>| match &ev {
                StreamEvent::TaskStart { node_id } => {
                    let from = last_node.as_deref().unwrap_or("START");
                    eprintln!("flow: {} → {}", from, node_id);
                    eprintln!("-------------------- {} --------------------", node_id);
                    last_node = Some(node_id.clone());
                }
                StreamEvent::Updates { node_id, state } => {
                    let label = match node_id.as_str() {
                        "think" => {
                            turn += 1;
                            format!("state after think (turn {})", turn)
                        }
                        "act" => "state after act".to_string(),
                        "observe" => "state after observe".to_string(),
                        _ => format!("state after {}", node_id),
                    };
                    eprintln!("--- {} ---", label);
                    eprintln!("{}", format_react_state_display(state, display_max_len));
                    if node_id == "think" && state.tool_calls.is_empty() {
                        eprintln!("(think → END: tool_calls empty, LLM gave FINAL_ANSWER)");
                    }
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
