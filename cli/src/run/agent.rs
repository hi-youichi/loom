//! Wraps loom::run_agent_with_options with stderr display callback.
//! Uses protocol format (type + payload) and optional envelope per protocol_spec.

use loom::{
    build_helve_config, build_react_run_context, run_agent_with_options, AnyStreamEvent, DupState, Envelope,
    GotState, ReActState, ToolCall, TotState,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};

use super::display::{
    format_dup_state_display, format_got_state_display, format_react_state_display,
    format_tot_state_display, truncate_display,
};
use crate::envelope::EnvelopeState;
use loom::{RunCmd, RunOptions, StreamEvent};

use super::RunError;

/// Single line when a node is entered (unified across agents).
fn log_node_enter(from: Option<&str>, node_id: &str) {
    let from = from.unwrap_or("START");
    eprintln!("Entering: {} (from {})", node_id, from);
}

/// Single line listing tool names (normal mode only; verbose shows full state).
fn log_tools_used(tool_calls: &[ToolCall]) {
    if tool_calls.is_empty() {
        return;
    }
    let names: Vec<&str> = tool_calls.iter().map(|tc| tc.name.as_str()).collect();
    eprintln!("tools: {}", names.join(", "));
}

/// Result of run_agent_wrapper: reply, optional events (when --json and no stream), optional envelope for reply line.
pub type RunAgentResult = Result<(String, Option<Vec<Value>>, Option<Envelope>), RunError>;

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
        let session_id = format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        if let Some(ref out) = stream_out {
            let out = out.clone();
            let state = Arc::new(Mutex::new(EnvelopeState::new(session_id.clone())));
            let state_clone = state.clone();
            let on_event = Box::new(move |ev: AnyStreamEvent| {
                let v = match state_clone.lock() {
                    Ok(mut s) => ev.to_protocol_format(&mut *s),
                    Err(_) => return,
                };
                let v = match v {
                    Ok(x) => x,
                    Err(e) => {
                        eprintln!("loom: failed to serialize stream event: {}", e);
                        serde_json::json!({ "type": "_error", "_serialize_error": format!("{}", e) })
                    }
                };
                if let Ok(mut f) = out.lock() {
                    f(v);
                }
            });
            let reply = run_agent_with_options(opts, cmd, Some(on_event)).await?;
            let reply_env = state.lock().map(|s| s.reply_envelope()).ok();
            return Ok((reply, None, reply_env));
        }
        let events: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let state = Arc::new(Mutex::new(EnvelopeState::new(session_id.clone())));
        let state_clone = state.clone();
        let on_event = Box::new(move |ev: AnyStreamEvent| {
            let v = match state_clone.lock() {
                Ok(mut s) => ev.to_protocol_format(&mut *s),
                Err(_) => return,
            };
            match v {
                Ok(value) => {
                    if let Ok(mut vec) = events_clone.lock() {
                        vec.push(value);
                    }
                }
                Err(e) => {
                    eprintln!("loom: failed to serialize stream event to JSON: {}", e);
                    if let Ok(mut vec) = events_clone.lock() {
                        vec.push(serde_json::json!({
                            "type": "_error",
                            "_serialize_error": format!("{}", e),
                        }));
                    }
                }
            }
        });
        let reply = run_agent_with_options(opts, cmd, Some(on_event)).await?;
        let events = events.lock().map(|v| v.clone()).unwrap_or_default();
        let reply_env = state.lock().map(|s| s.reply_envelope()).ok();
        return Ok((reply, Some(events), reply_env));
    }

    let state = Arc::new(Mutex::new(EventState {
        turn: 0,
        last_node: None,
    }));

    let state_clone = state.clone();
    let verbose = opts.verbose;
    let on_event = Box::new(move |ev: AnyStreamEvent| {
        let mut s = state_clone.lock().unwrap();
        match &ev {
            AnyStreamEvent::React(e) => on_event_react(e, &mut *s, display_max_len, verbose),
            AnyStreamEvent::Dup(e) => on_event_dup(e, &mut *s, display_max_len, verbose),
            AnyStreamEvent::Tot(e) => on_event_tot(e, &mut *s, display_max_len, verbose),
            AnyStreamEvent::Got(e) => on_event_got(e, &mut *s, display_max_len, verbose),
        }
    });

    let reply = run_agent_with_options(opts, cmd, Some(on_event)).await?;

    if verbose {
        if let Some(ref from) = state.lock().unwrap().last_node {
            eprintln!("flow: {} → END", from);
        }
    }
    Ok((reply, None, None))
}

fn on_event_react(
    ev: &StreamEvent<ReActState>,
    s: &mut EventState,
    display_max_len: usize,
    verbose: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id } => {
            log_node_enter(s.last_node.as_deref(), node_id);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::Messages { chunk, .. } => {
            print!("{}", chunk.content);
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        StreamEvent::Updates { node_id, state } => {
            if verbose {
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
            } else if node_id == "think" && !state.tool_calls.is_empty() {
                log_tools_used(&state.tool_calls);
            }
        }
        _ => {}
    }
}

fn on_event_dup(
    ev: &StreamEvent<DupState>,
    s: &mut EventState,
    display_max_len: usize,
    verbose: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id } => {
            log_node_enter(s.last_node.as_deref(), node_id);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::Messages { chunk, .. } => {
            print!("{}", chunk.content);
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        StreamEvent::Updates { node_id, state } => {
            if verbose {
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
            } else {
                if node_id == "plan" {
                    s.turn += 1;
                    if !state.core.tool_calls.is_empty() {
                        log_tools_used(&state.core.tool_calls);
                    }
                }
            }
        }
        _ => {}
    }
}

fn on_event_tot(
    ev: &StreamEvent<TotState>,
    s: &mut EventState,
    display_max_len: usize,
    verbose: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id } => {
            log_node_enter(s.last_node.as_deref(), node_id);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::TotExpand { candidates } => {
            if verbose {
                eprintln!("--- ToT expand: {} candidates ---", candidates.len());
                for (i, c) in candidates.iter().enumerate() {
                    eprintln!("  [{}] {}", i + 1, c);
                }
            }
        }
        StreamEvent::TotEvaluate { chosen, scores } => {
            if verbose {
                eprintln!(
                    "--- ToT evaluate: chosen={}, scores={:?} ---",
                    chosen, scores
                );
            }
        }
        StreamEvent::TotBacktrack { reason, to_depth } => {
            if verbose {
                eprintln!(
                    "--- ToT backtrack: reason={}, to_depth={} ---",
                    reason, to_depth
                );
            }
        }
        StreamEvent::Messages { chunk, .. } => {
            print!("{}", chunk.content);
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        StreamEvent::Updates { node_id, state } => {
            if verbose {
                let label = match node_id.as_str() {
                    "think_expand" => "state after think_expand".to_string(),
                    "think_evaluate" => "state after think_evaluate".to_string(),
                    "act" => "state after act".to_string(),
                    "observe" => "state after observe".to_string(),
                    _ => format!("state after {}", node_id),
                };
                eprintln!("--- {} ---", label);
                eprintln!("{}", format_tot_state_display(state, display_max_len));
            } else if node_id == "act" && !state.core.tool_calls.is_empty() {
                log_tools_used(&state.core.tool_calls);
            }
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

fn on_event_got(
    ev: &StreamEvent<GotState>,
    s: &mut EventState,
    display_max_len: usize,
    verbose: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id } => {
            log_node_enter(s.last_node.as_deref(), node_id);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::GotPlan {
            node_count,
            edge_count,
            node_ids,
        } => {
            if verbose {
                eprintln!(
                    "--- GoT plan: {} nodes, {} edges ---",
                    node_count, edge_count
                );
                for id in node_ids {
                    eprintln!("  node: {}", id);
                }
            }
        }
        StreamEvent::GotNodeStart { node_id } => {
            if verbose {
                eprintln!("--- GoT node start: {} ---", node_id);
            }
        }
        StreamEvent::GotNodeComplete {
            node_id,
            result_summary,
        } => {
            if verbose {
                eprintln!("--- GoT node complete: {} ---", node_id);
                eprintln!("  result: {}", result_summary);
            }
        }
        StreamEvent::GotNodeFailed { node_id, error } => {
            if verbose {
                eprintln!("--- GoT node failed: {} ---", node_id);
                eprintln!("  error: {}", error);
            }
        }
        StreamEvent::GotExpand {
            node_id,
            nodes_added,
            edges_added,
        } => {
            if verbose {
                eprintln!(
                    "--- AGoT expand: {} → +{} nodes, +{} edges ---",
                    node_id, nodes_added, edges_added
                );
            }
        }
        StreamEvent::Messages { chunk, .. } => {
            print!("{}", chunk.content);
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        StreamEvent::Updates { node_id, state } => {
            if verbose {
                eprintln!("--- state after {} ---", node_id);
                eprintln!("{}", format_got_state_display(state, display_max_len));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom::{
        GotRunnerConfig, Message, TaskGraph, TaskNode, TaskNodeState, TaskStatus, ToolCall,
        TotExtension, TotRunnerConfig, UnderstandOutput,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    fn react_state() -> ReActState {
        ReActState {
            messages: vec![Message::user("hi"), Message::Assistant("hello".into())],
            ..ReActState::default()
        }
    }

    fn minimal_build_config() -> loom::ReactBuildConfig {
        loom::ReactBuildConfig {
            db_path: None,
            thread_id: None,
            user_id: None,
            system_prompt: None,
            exa_api_key: None,
            twitter_api_key: None,
            mcp_exa_url: "https://mcp.exa.ai/mcp".to_string(),
            mcp_remote_cmd: "npx".to_string(),
            mcp_remote_args: "-y mcp-remote".to_string(),
            mcp_verbose: false,
            openai_api_key: None,
            openai_base_url: None,
            model: None,
            embedding_api_key: None,
            embedding_base_url: None,
            embedding_model: None,
            working_folder: None,
            approval_policy: None,
            compaction_config: None,
            tot_config: TotRunnerConfig::default(),
            got_config: GotRunnerConfig::default(),
        }
    }

    #[test]
    fn any_stream_event_to_format_a_and_protocol_format() {
        let ev = AnyStreamEvent::React(StreamEvent::TaskStart {
            node_id: "think".to_string(),
        });
        let a = ev.to_format_a().unwrap();
        assert!(a.get("TaskStart").is_some());

        let mut state = EnvelopeState::new("sess-1".to_string());
        let p = ev.to_protocol_format(&mut state).unwrap();
        assert_eq!(p["type"], "node_enter");
        assert_eq!(p["id"], "think");
        assert_eq!(p["session_id"], "sess-1");
        assert_eq!(p["event_id"], 1);
    }

    #[test]
    fn on_event_react_updates_last_node_and_turn() {
        let mut s = EventState {
            turn: 0,
            last_node: None,
        };
        on_event_react(
            &StreamEvent::TaskStart {
                node_id: "think".to_string(),
            },
            &mut s,
            100,
            true,
        );
        assert_eq!(s.last_node.as_deref(), Some("think"));

        on_event_react(
            &StreamEvent::Updates {
                node_id: "think".to_string(),
                state: react_state(),
            },
            &mut s,
            100,
            true,
        );
        assert_eq!(s.turn, 1);
    }

    #[test]
    fn on_event_dup_and_tot_and_got_do_not_panic() {
        let mut s = EventState {
            turn: 0,
            last_node: None,
        };

        let dup_state = DupState {
            core: react_state(),
            understood: None,
        };
        on_event_dup(
            &StreamEvent::TaskStart {
                node_id: "understand".to_string(),
            },
            &mut s,
            120,
            true,
        );
        on_event_dup(
            &StreamEvent::Updates {
                node_id: "plan".to_string(),
                state: dup_state,
            },
            &mut s,
            120,
            true,
        );
        assert_eq!(s.turn, 1);

        let tot_state = TotState {
            core: react_state(),
            tot: TotExtension::default(),
        };
        on_event_tot(
            &StreamEvent::TaskStart {
                node_id: "think_expand".to_string(),
            },
            &mut s,
            120,
            true,
        );
        on_event_tot(
            &StreamEvent::TotExpand {
                candidates: vec!["a".to_string(), "b".to_string()],
            },
            &mut s,
            120,
            true,
        );
        on_event_tot(
            &StreamEvent::Updates {
                node_id: "observe".to_string(),
                state: tot_state,
            },
            &mut s,
            120,
            true,
        );

        let got_state = GotState {
            input_message: "q".to_string(),
            task_graph: TaskGraph {
                nodes: vec![TaskNode {
                    id: "n1".to_string(),
                    description: "d1".to_string(),
                    tool_calls: vec![ToolCall {
                        name: "search".to_string(),
                        arguments: "{}".to_string(),
                        id: None,
                    }],
                }],
                edges: vec![],
            },
            node_states: [(
                "n1".to_string(),
                TaskNodeState {
                    status: TaskStatus::Done,
                    result: Some("ok".to_string()),
                    error: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        on_event_got(
            &StreamEvent::TaskStart {
                node_id: "plan_graph".to_string(),
            },
            &mut s,
            120,
            true,
        );
        on_event_got(
            &StreamEvent::GotPlan {
                node_count: 1,
                edge_count: 0,
                node_ids: vec!["n1".to_string()],
            },
            &mut s,
            120,
            true,
        );
        on_event_got(
            &StreamEvent::Updates {
                node_id: "execute_graph".to_string(),
                state: got_state,
            },
            &mut s,
            120,
            true,
        );
        assert_eq!(s.last_node.as_deref(), Some("plan_graph"));
    }

    #[test]
    fn log_tools_used_handles_empty_and_non_empty() {
        log_tools_used(&[]);
        log_tools_used(&[ToolCall {
            name: "search".to_string(),
            arguments: "{}".to_string(),
            id: None,
        }]);
    }

    #[test]
    fn non_verbose_paths_update_turns_without_panics() {
        let mut s = EventState {
            turn: 0,
            last_node: None,
        };
        let react_with_tool = ReActState {
            tool_calls: vec![ToolCall {
                name: "bash".to_string(),
                arguments: "{\"command\":\"echo hi\"}".to_string(),
                id: None,
            }],
            ..react_state()
        };
        on_event_react(
            &StreamEvent::Updates {
                node_id: "think".to_string(),
                state: react_with_tool,
            },
            &mut s,
            120,
            false,
        );
        assert_eq!(s.turn, 0);

        let dup_state = DupState {
            core: ReActState {
                tool_calls: vec![ToolCall {
                    name: "read".to_string(),
                    arguments: "{}".to_string(),
                    id: None,
                }],
                ..react_state()
            },
            understood: Some(UnderstandOutput {
                core_goal: "goal".to_string(),
                key_constraints: vec!["c1".to_string()],
                relevant_context: "ctx".to_string(),
            }),
        };
        on_event_dup(
            &StreamEvent::Updates {
                node_id: "plan".to_string(),
                state: dup_state,
            },
            &mut s,
            120,
            false,
        );
        assert_eq!(s.turn, 1);
    }

    #[test]
    fn verbose_tot_and_got_event_variants_are_handled() {
        let mut s = EventState {
            turn: 0,
            last_node: None,
        };

        on_event_tot(
            &StreamEvent::TotEvaluate {
                chosen: 0,
                scores: vec![0.9],
            },
            &mut s,
            80,
            true,
        );
        on_event_tot(
            &StreamEvent::TotBacktrack {
                reason: "retry".to_string(),
                to_depth: 2,
            },
            &mut s,
            80,
            true,
        );
        on_event_tot(
            &StreamEvent::Messages {
                chunk: loom::MessageChunk {
                    content: "tok".to_string(),
                },
                metadata: loom::StreamMetadata {
                    loom_node: "think_expand".to_string(),
                },
            },
            &mut s,
            80,
            true,
        );

        on_event_got(
            &StreamEvent::GotNodeStart {
                node_id: "n1".to_string(),
            },
            &mut s,
            80,
            true,
        );
        on_event_got(
            &StreamEvent::GotNodeComplete {
                node_id: "n1".to_string(),
                result_summary: "done".to_string(),
            },
            &mut s,
            80,
            true,
        );
        on_event_got(
            &StreamEvent::GotNodeFailed {
                node_id: "n2".to_string(),
                error: "boom".to_string(),
            },
            &mut s,
            80,
            true,
        );
        on_event_got(
            &StreamEvent::GotExpand {
                node_id: "n1".to_string(),
                nodes_added: 2,
                edges_added: 1,
            },
            &mut s,
            80,
            true,
        );
        on_event_got(
            &StreamEvent::Messages {
                chunk: loom::MessageChunk {
                    content: "chunk".to_string(),
                },
                metadata: loom::StreamMetadata {
                    loom_node: "execute_graph".to_string(),
                },
            },
            &mut s,
            80,
            true,
        );
    }

    #[tokio::test]
    async fn print_loaded_tools_succeeds_with_minimal_config() {
        let cfg = minimal_build_config();
        let res = print_loaded_tools(&cfg).await;
        assert!(res.is_ok());
    }

    fn invalid_opts(output_json: bool) -> RunOptions {
        RunOptions {
            message: "hello".to_string(),
            // Deterministic failure path in build context (invalid file-tool root).
            working_folder: Some(PathBuf::from(
                "/definitely/not/exist/loom-cli-run-agent-tests",
            )),
            thread_id: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 200,
            output_json,
        }
    }

    #[tokio::test]
    async fn run_agent_wrapper_errors_for_invalid_working_folder_plain_mode() {
        let res = run_agent_wrapper(&invalid_opts(false), &RunCmd::React, None).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn run_agent_wrapper_errors_for_invalid_working_folder_json_collect_mode() {
        let res = run_agent_wrapper(&invalid_opts(true), &RunCmd::React, None).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn run_agent_wrapper_errors_for_invalid_working_folder_json_stream_mode() {
        let sink: Arc<Mutex<dyn FnMut(Value) + Send>> = Arc::new(Mutex::new(|_v: Value| {}));
        let res = run_agent_wrapper(&invalid_opts(true), &RunCmd::React, Some(sink)).await;
        assert!(res.is_err());
    }
}
