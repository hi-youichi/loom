//! Unified agent runner: ReAct, DUP, ToT, GoT.
//!
//! Single entry point `run_agent(opts, cmd)` that builds the appropriate runner,
//! streams events with type-specific on_event handlers, and returns the final reply.

use graphweave::{DupRunner, DupState, GotRunner, GotState, ReactRunner, ReActState, StreamEvent, TotRunner, TotState};
use tracing::{info_span, Instrument};

use super::{
    build_helve_config, build_runner_from_cli,
    format_dup_state_display, format_got_state_display, format_react_state_display,
    format_tot_state_display, truncate_display,
    print_loaded_tools, RunError, RunOptions,
};

/// Command mode for running an agent (internal representation).
#[derive(Clone, Debug)]
pub enum RunCmd {
    React,
    Dup,
    Tot,
    Got { got_adaptive: bool },
}

/// Type-erased runner for any agent pattern.
pub enum AnyRunner {
    React(ReactRunner),
    Dup(DupRunner),
    Tot(TotRunner),
    Got(GotRunner),
}

/// Runs the agent (React, Dup, Tot, or Got) based on `cmd`.
pub async fn run_agent(opts: &RunOptions, cmd: &RunCmd) -> Result<String, RunError> {
    let (helve, mut config) = build_helve_config(opts);
    let thread_id_log = config.thread_id.as_deref().unwrap_or("").to_string();
    let kind = match cmd {
        RunCmd::React => "react",
        RunCmd::Dup => "dup",
        RunCmd::Tot => "tot",
        RunCmd::Got { .. } => "got",
    };
    let span = info_span!("run", kind = kind, thread_id = %thread_id_log);
    tracing::info!(parent: &span, thread_id = %thread_id_log, "run started");

    if helve.role_setting.is_some() {
        eprintln!("SOUL.md loaded; system prompt (including it) is in state.messages[0].");
    }
    if helve.agents_md.is_some() {
        eprintln!("AGENTS.md loaded; included in system prompt.");
    }
    print_loaded_tools(&config).await?;

    let runner = build_runner_from_cli(&helve, &mut config, opts, cmd)
        .instrument(span.clone())
        .await?;

    let display_max_len = opts.display_max_len;
    let mut turn = 0u32;
    let mut last_node: Option<String> = None;

    let reply = match &runner {
        AnyRunner::React(r) => {
            let state = r
                .stream_with_config(
                    opts.message.as_str(),
                    None,
                    Some(|ev: StreamEvent<ReActState>| {
                        on_event_react(ev, &mut turn, &mut last_node, display_max_len)
                    }),
                )
                .instrument(span.clone())
                .await?;
            state.last_assistant_reply().unwrap_or_default()
        }
        AnyRunner::Dup(r) => {
            let state = r
                .stream_with_config(
                    opts.message.as_str(),
                    None,
                    Some(|ev: StreamEvent<DupState>| {
                        on_event_dup(ev, &mut turn, &mut last_node, display_max_len)
                    }),
                )
                .instrument(span.clone())
                .await?;
            state.last_assistant_reply().unwrap_or_default()
        }
        AnyRunner::Tot(r) => {
            let state = r
                .stream_with_config(
                    opts.message.as_str(),
                    None,
                    Some(|ev: StreamEvent<TotState>| {
                        on_event_tot(ev, &mut last_node, display_max_len)
                    }),
                )
                .instrument(span.clone())
                .await?;
            state.last_assistant_reply().unwrap_or_default()
        }
        AnyRunner::Got(r) => {
            let state = r
                .stream_with_config(
                    opts.message.as_str(),
                    None,
                    Some(|ev: StreamEvent<GotState>| {
                        on_event_got(ev, &mut last_node, display_max_len)
                    }),
                )
                .instrument(span.clone())
                .await?;
            eprintln!("--- GoT final state ---");
            eprintln!("{}", format_got_state_display(&state, display_max_len));
            state.summary_result()
        }
    };

    if let Some(from) = last_node.as_deref() {
        eprintln!("flow: {} → END", from);
    }
    Ok(reply)
}

fn on_event_react(
    ev: StreamEvent<ReActState>,
    turn: &mut u32,
    last_node: &mut Option<String>,
    display_max_len: usize,
) {
    match &ev {
        StreamEvent::TaskStart { node_id } => {
            let from = last_node.as_deref().unwrap_or("START");
            eprintln!("flow: {} → {}", from, node_id);
            eprintln!("-------------------- {} --------------------", node_id);
            *last_node = Some(node_id.clone());
        }
        StreamEvent::Updates { node_id, state } => {
            let label = match node_id.as_str() {
                "think" => {
                    *turn += 1;
                    format!("state after think (turn {})", *turn)
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
    }
}

fn on_event_dup(
    ev: StreamEvent<DupState>,
    turn: &mut u32,
    last_node: &mut Option<String>,
    display_max_len: usize,
) {
    match &ev {
        StreamEvent::TaskStart { node_id } => {
            let from = last_node.as_deref().unwrap_or("START");
            eprintln!("flow: {} → {}", from, node_id);
            eprintln!("-------------------- {} --------------------", node_id);
            *last_node = Some(node_id.clone());
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
                    *turn += 1;
                    format!("state after plan (turn {})", *turn)
                }
                "act" => "state after act".to_string(),
                "observe" => "state after observe".to_string(),
                _ => format!("state after {}", node_id),
            };
            eprintln!("--- {} ---", label);
            eprintln!("{}", format_dup_state_display(state, display_max_len));
        }
        _ => {}
    }
}

fn on_event_tot(
    ev: StreamEvent<TotState>,
    last_node: &mut Option<String>,
    display_max_len: usize,
) {
    match &ev {
        StreamEvent::TaskStart { node_id } => {
            let from = last_node.as_deref().unwrap_or("START");
            eprintln!("flow: {} → {}", from, node_id);
            eprintln!("-------------------- {} --------------------", node_id);
            *last_node = Some(node_id.clone());
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
    }
}

fn on_event_got(
    ev: StreamEvent<GotState>,
    last_node: &mut Option<String>,
    display_max_len: usize,
) {
    match &ev {
        StreamEvent::TaskStart { node_id } => {
            let from = last_node.as_deref().unwrap_or("START");
            eprintln!("flow: {} → {}", from, node_id);
            eprintln!("-------------------- {} --------------------", node_id);
            *last_node = Some(node_id.clone());
        }
        StreamEvent::GotPlan {
            node_count,
            edge_count,
            node_ids,
        } => {
            eprintln!("--- GoT plan: {} nodes, {} edges ---", node_count, edge_count);
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
    }
}
