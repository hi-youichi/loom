//! Wraps loom::run_agent with stderr display callback.

use loom::{
    build_helve_config, build_react_run_context, run_agent, AnyStreamEvent, DupState, GotState,
    ReActState, TotState,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};

use super::display::{
    format_dup_state_display, format_got_state_display, format_react_state_display,
    format_tot_state_display, truncate_display,
};
use loom::{RunCmd, RunOptions, StreamEvent};

use super::RunError;

/// Result of run_agent_wrapper: reply text and optionally collected stream events as JSON (when --json).
pub type RunAgentResult = Result<(String, Option<Vec<Value>>), RunError>;

/// Runs the agent with stderr display for stream events.
/// When `opts.output_json` is true: if `stream_out` is Some, each event is written via it and returns (reply, None);
/// otherwise collects all events and returns (reply, Some(events)).
pub async fn run_agent_wrapper(
    opts: &RunOptions,
    cmd: &RunCmd,
    stream_out: Option<Arc<Mutex<dyn FnMut(Value) + Send>>>,
) -> RunAgentResult {
    let (helve, config) = build_helve_config(opts);
    if !opts.output_json {
        if helve.role_setting.is_some() {
            eprintln!("SOUL.md loaded; system prompt (including it) is in state.messages[0].");
        }
        if helve.agents_md.is_some() {
            eprintln!("AGENTS.md loaded; included in system prompt.");
        }
    }
    print_loaded_tools(&config).await?;

    let display_max_len = opts.display_max_len;

    if opts.output_json {
        if let Some(ref out) = stream_out {
            let out = out.clone();
            let on_event = Box::new(move |ev: AnyStreamEvent| {
                if let Ok(v) = ev.to_format_a() {
                    if let Ok(mut f) = out.lock() {
                        f(v);
                    }
                }
            });
            let reply = run_agent(opts, cmd, Some(on_event)).await?;
            return Ok((reply, None));
        }
        let events: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let on_event = Box::new(move |ev: AnyStreamEvent| {
            match ev.to_format_a() {
                Ok(v) => {
                    if let Ok(mut vec) = events_clone.lock() {
                        vec.push(v);
                    }
                }
                Err(e) => {
                    eprintln!("loom: failed to serialize stream event to JSON: {}", e);
                    if let Ok(mut vec) = events_clone.lock() {
                        vec.push(serde_json::json!({
                            "_serialize_error": format!("{}", e),
                        }));
                    }
                }
            }
        });
        let reply = run_agent(opts, cmd, Some(on_event)).await?;
        let events = events.lock().map(|v| v.clone()).unwrap_or_default();
        return Ok((reply, Some(events)));
    }

    let state = Arc::new(Mutex::new(EventState {
        turn: 0,
        last_node: None,
    }));

    let state_clone = state.clone();
    let on_event = Box::new(move |ev: AnyStreamEvent| {
        let mut s = state_clone.lock().unwrap();
        match &ev {
            AnyStreamEvent::React(e) => on_event_react(e, &mut *s, display_max_len),
            AnyStreamEvent::Dup(e) => on_event_dup(e, &mut *s, display_max_len),
            AnyStreamEvent::Tot(e) => on_event_tot(e, &mut *s, display_max_len),
            AnyStreamEvent::Got(e) => on_event_got(e, &mut *s, display_max_len),
        }
    });

    let reply = run_agent(opts, cmd, Some(on_event)).await?;

    if let Some(ref from) = state.lock().unwrap().last_node {
        eprintln!("flow: {} → END", from);
    }
    Ok((reply, None))
}

fn on_event_react(ev: &StreamEvent<ReActState>, s: &mut EventState, display_max_len: usize) {
    match ev {
        StreamEvent::TaskStart { node_id } => {
            let from = s.last_node.as_deref().unwrap_or("START");
            eprintln!("flow: {} → {}", from, node_id);
            eprintln!("-------------------- {} --------------------", node_id);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::Updates { node_id, state } => {
            let label = match node_id.as_str() {
                "think" => {
                    s.turn += 1;
                    format!("state after think (turn {})", s.turn)
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

fn on_event_dup(ev: &StreamEvent<DupState>, s: &mut EventState, display_max_len: usize) {
    match ev {
        StreamEvent::TaskStart { node_id } => {
            let from = s.last_node.as_deref().unwrap_or("START");
            eprintln!("flow: {} → {}", from, node_id);
            eprintln!("-------------------- {} --------------------", node_id);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::Updates { node_id, state } => {
            match node_id.as_str() {
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
                }
                "plan" => s.turn += 1,
                _ => {}
            }
            eprintln!("--- state after {} ---", node_id);
            eprintln!("{}", format_dup_state_display(state, display_max_len));
        }
        _ => {}
    }
}

fn on_event_tot(ev: &StreamEvent<TotState>, s: &mut EventState, display_max_len: usize) {
    match ev {
        StreamEvent::TaskStart { node_id } => {
            let from = s.last_node.as_deref().unwrap_or("START");
            eprintln!("flow: {} → {}", from, node_id);
            eprintln!("-------------------- {} --------------------", node_id);
            s.last_node = Some(node_id.clone());
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

struct EventState {
    turn: u32,
    last_node: Option<String>,
}

async fn print_loaded_tools(config: &loom::ReactBuildConfig) -> Result<(), RunError> {
    let ctx = build_react_run_context(config)
        .await
        .map_err(|e| RunError::Build(loom::BuildRunnerError::Context(e)))?;
    let tools = ctx.tool_source.list_tools().await.map_err(|e| {
        RunError::Build(loom::BuildRunnerError::Context(
            loom::AgentError::ExecutionFailed(e.to_string()),
        ))
    })?;
    let names: Vec<&str> = tools.iter().map(|s| s.name.as_str()).collect();
    eprintln!("loaded tools: {}", names.join(", "));
    Ok(())
}

fn on_event_got(ev: &StreamEvent<GotState>, s: &mut EventState, display_max_len: usize) {
    match ev {
        StreamEvent::TaskStart { node_id } => {
            let from = s.last_node.as_deref().unwrap_or("START");
            eprintln!("flow: {} → {}", from, node_id);
            eprintln!("-------------------- {} --------------------", node_id);
            s.last_node = Some(node_id.clone());
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
