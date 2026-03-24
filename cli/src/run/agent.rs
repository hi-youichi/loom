//! Wraps loom::run_agent_with_options with stderr display callback.
//! Uses protocol format (type + payload) and optional envelope per protocol_spec.

use chrono::Local;
use loom::{
    build_helve_config, build_react_run_context, list_available_profiles, run_agent_with_options,
    AnyStreamEvent, DupState, Envelope, GotState, MessageChunkKind, ModelLimitResolver,
    ModelsDevResolver, ReActState, ResolvedAgent, ToolCall, TotState,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::display::{
    format_dup_state_display, format_got_state_display, format_react_state_display,
    format_tot_state_display, truncate_display,
};
use crate::envelope::EnvelopeState;
use crate::backend::RunStopReason;
use loom::{RunCmd, RunOptions, StreamEvent};

use super::RunError;

fn completion_reply(result: loom::RunCompletion) -> (String, Option<String>, RunStopReason) {
    match result {
        loom::RunCompletion::Finished(result) => {
            (result.reply, result.reasoning_content, RunStopReason::EndTurn)
        }
        loom::RunCompletion::Cancelled => (String::new(), None, RunStopReason::Cancelled),
    }
}

/// Prints agent profile info to stderr at startup.
fn print_agent_banner(resolved: &Option<ResolvedAgent>) {
    match resolved {
        Some(ra) => {
            let desc = ra
                .description
                .as_deref()
                .map(|d| format!(" — {}", d))
                .unwrap_or_default();
            eprintln!("agent: {} ({}){}", ra.name, ra.source, desc);
        }
        None => eprintln!("agent: (none)"),
    }
}

/// Prints current local time to stderr (when --timestamp is set, before each reply).
pub fn print_reply_timestamp() {
    eprintln!("{}", Local::now().format("%Y-%m-%d %H:%M:%S"));
}

/// Prints available agent names to stderr (use -P/--agent to switch).
fn print_available_agents() {
    let profiles = list_available_profiles();
    if profiles.is_empty() {
        return;
    }
    let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
    eprintln!("available agents: {} (use -P/--agent to switch)", names.join(", "));
}

/// Single line when a node is entered (unified across agents).
fn log_node_enter(from: Option<&str>, node_id: &str, verbose: bool) {
    if !verbose {
        return;
    }
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

/// Formats context limit for display (e.g., "128K" or "1.5M").
fn format_context_limit(limit: u32) -> String {
    if limit >= 1_000_000 {
        format!("{:.1}M", limit as f64 / 1_000_000.0)
    } else if limit >= 1000 {
        format!("{}K", limit / 1000)
    } else {
        limit.to_string()
    }
}

/// Prints model name and context limit to stderr at startup.
async fn print_model_info(model: Option<&String>) {
    let model_name = match model {
        Some(m) if !m.is_empty() => m.as_str(),
        _ => {
            eprintln!("model: (default)");
            return;
        }
    };

    // Try to resolve context limit from models.dev
    let resolver = ModelsDevResolver::new();
    match resolver.resolve_combined(model_name).await {
        Some(spec) => {
            eprintln!(
                "model: {} ({} context)",
                model_name,
                format_context_limit(spec.context_limit)
            );
        }
        None => {
            // Log detailed reason why resolution failed
            tracing::debug!(
                "Model spec resolution failed for '{}'. \
                 This usually means: \
                 1) The model name doesn't include a provider prefix (e.g., 'glm-5' instead of 'zai/glm-5'), \
                 2) The provider/model combination is not in the models.dev database, or \
                 3) Network error when fetching from models.dev", 
                model_name
            );
            eprintln!("model: {} (context: unknown)", model_name);
        }
    }
}

#[derive(Debug)]
pub struct RunAgentOutput {
    pub reply: String,
    pub reasoning_content: Option<String>,
    pub events: Option<Vec<Value>>,
    pub reply_envelope: Option<Envelope>,
    pub stop_reason: RunStopReason,
}

/// Result of run_agent_wrapper.
pub type RunAgentResult = Result<RunAgentOutput, RunError>;

/// Runs the agent with stderr display for stream events.
/// When `opts.output_json` is true: if `stream_out` is Some, each event is written via it and returns (reply, None);
/// otherwise collects all events and returns (reply, Some(events)).
pub async fn run_agent_wrapper(
    opts: &RunOptions,
    cmd: &RunCmd,
    stream_out: Option<Arc<Mutex<dyn FnMut(Value) + Send>>>,
) -> RunAgentResult {
    let (helve, config, resolved_agent) = build_helve_config(opts);
    if !opts.output_json {
        if opts.dry_run {
            eprintln!("dry run: tools will not be executed");
        }
        print_agent_banner(&resolved_agent);
        print_available_agents();
        if helve.role_setting.is_some() {
            eprintln!("instructions/role loaded; system prompt (including it) is in state.messages[0].");
        }
        if helve.agents_md.is_some() {
            eprintln!("AGENTS.md loaded; included in system prompt.");
        }
        print_model_info(config.model.as_ref()).await;
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
            let result = run_agent_with_options(opts, cmd, Some(on_event)).await?;
            let reply_env = state.lock().map(|s| s.reply_envelope()).ok();
            let (reply, reasoning_content, stop_reason) = completion_reply(result);
            return Ok(RunAgentOutput {
                reply,
                reasoning_content,
                events: None,
                reply_envelope: reply_env,
                stop_reason,
            });
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
        let result = run_agent_with_options(opts, cmd, Some(on_event)).await?;
        let events = events.lock().map(|v| v.clone()).unwrap_or_default();
        let reply_env = state.lock().map(|s| s.reply_envelope()).ok();
        let (reply, reasoning_content, stop_reason) = completion_reply(result);
        return Ok(RunAgentOutput {
            reply,
            reasoning_content,
            events: Some(events),
            reply_envelope: reply_env,
            stop_reason,
        });
    }

    let agent_display = resolved_agent.as_ref().map(|ra| format!("{} ({})", ra.name, ra.source));
    let state = Arc::new(Mutex::new(EventState {
        turn: 0,
        last_node: None,
        reply_started: false,
        agent_display,
        total_prompt_tokens: 0,
        total_completion_tokens: 0,
    }));

    let state_clone = state.clone();
    let verbose = opts.verbose;
    let output_timestamp = opts.output_timestamp;
    let on_event = Box::new(move |ev: AnyStreamEvent| {
        let mut s = state_clone.lock().unwrap();
        match &ev {
            AnyStreamEvent::React(e) => {
                on_event_react(e, &mut *s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Dup(e) => {
                on_event_dup(e, &mut *s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Tot(e) => {
                on_event_tot(e, &mut *s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Got(e) => {
                on_event_got(e, &mut *s, display_max_len, verbose, output_timestamp)
            }
        }
    });

    let start = Instant::now();
    let result = run_agent_with_options(opts, cmd, Some(on_event)).await?;
    let duration = start.elapsed();

    if verbose {
        if let Some(ref from) = state.lock().unwrap().last_node {
            eprintln!("flow: {} → END", from);
        }
    }
    if let Ok(s) = state.lock() {
        let total_tokens = s.total_prompt_tokens as u64 + s.total_completion_tokens as u64;
        let secs = duration.as_secs_f64();
        let tokens_per_sec = if secs > 0.0 {
            total_tokens as f64 / secs
        } else {
            0.0
        };
        eprintln!(
            "LLM: {:.2}s, {:.0} tokens/s (prompt: {}, completion: {})",
            secs, tokens_per_sec, s.total_prompt_tokens, s.total_completion_tokens
        );
    }
    let (reply, reasoning_content, stop_reason) = completion_reply(result);
    Ok(RunAgentOutput {
        reply,
        reasoning_content,
        events: None,
        reply_envelope: None,
        stop_reason,
    })
}

fn print_stream_chunk(chunk: &loom::MessageChunk) {
    if chunk.kind == MessageChunkKind::Thinking {
        eprint!("{}", chunk.content);
        let _ = std::io::Write::flush(&mut std::io::stderr());
    } else {
        print!("{}", chunk.content);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

fn on_event_react(
    ev: &StreamEvent<ReActState>,
    s: &mut EventState,
    display_max_len: usize,
    verbose: bool,
    output_timestamp: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id, .. } => {
            if node_id == "think" {
                eprintln!("Think");
            }
            log_node_enter(s.last_node.as_deref(), node_id, verbose);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::Messages { chunk, .. } => {
            if !s.reply_started {
                if let Some(ref ad) = s.agent_display {
                    eprintln!("AGENT: {}", ad);
                }
                if output_timestamp {
                    print_reply_timestamp();
                }
                s.reply_started = true;
            }
            print_stream_chunk(chunk);
        }
        StreamEvent::Updates { node_id, state, .. } => {
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
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            prefill_duration,
            decode_duration,
            ..
        } => {
            s.total_prompt_tokens = s.total_prompt_tokens.saturating_add(*prompt_tokens);
            s.total_completion_tokens = s.total_completion_tokens.saturating_add(*completion_tokens);

            match (prefill_duration, decode_duration) {
                (Some(prefill), Some(decode)) => {
                    let prefill_secs = prefill.as_secs_f64();
                    let decode_secs = decode.as_secs_f64();
                    let total_secs = prefill_secs + decode_secs;
                    let prefill_rate = if prefill_secs > 0.0 {
                        *prompt_tokens as f64 / prefill_secs
                    } else {
                        0.0
                    };
                    let decode_rate = if decode_secs > 0.0 {
                        *completion_tokens as f64 / decode_secs
                    } else {
                        0.0
                    };
                    eprintln!(
                        "\nLLM: {:.2}s | prefill: {}t / {:.2}s = {:.0} t/s | decode: {}t / {:.2}s = {:.0} t/s",
                        total_secs,
                        prompt_tokens, prefill_secs, prefill_rate,
                        completion_tokens, decode_secs, decode_rate
                    );
                }
                _ => {
                    eprintln!(
                        "\nLLM: prompt={}, completion={}",
                        prompt_tokens, completion_tokens
                    );
                }
            }

            tracing::info!(
                prompt_tokens,
                completion_tokens,
                total_tokens = *prompt_tokens + *completion_tokens,
                "LLM usage"
            );
        }
        _ => {}
    }
}

fn on_event_dup(
    ev: &StreamEvent<DupState>,
    s: &mut EventState,
    display_max_len: usize,
    verbose: bool,
    output_timestamp: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id, .. } => {
            log_node_enter(s.last_node.as_deref(), node_id, verbose);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::Messages { chunk, .. } => {
            if !s.reply_started {
                if let Some(ref ad) = s.agent_display {
                    eprintln!("AGENT: {}", ad);
                }
                if output_timestamp {
                    print_reply_timestamp();
                }
                s.reply_started = true;
            }
            print_stream_chunk(chunk);
        }
        StreamEvent::Updates { node_id, state, .. } => {
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
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            ..
        } => {
            s.total_prompt_tokens = s.total_prompt_tokens.saturating_add(*prompt_tokens);
            s.total_completion_tokens = s.total_completion_tokens.saturating_add(*completion_tokens);
            tracing::info!(
                prompt_tokens,
                completion_tokens,
                total_tokens = *prompt_tokens + *completion_tokens,
                "LLM usage"
            );
        }
        _ => {}
    }
}

fn on_event_tot(
    ev: &StreamEvent<TotState>,
    s: &mut EventState,
    display_max_len: usize,
    verbose: bool,
    output_timestamp: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id, .. } => {
            log_node_enter(s.last_node.as_deref(), node_id, verbose);
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
            if !s.reply_started {
                if let Some(ref ad) = s.agent_display {
                    eprintln!("AGENT: {}", ad);
                }
                if output_timestamp {
                    print_reply_timestamp();
                }
                s.reply_started = true;
            }
            print_stream_chunk(chunk);
        }
        StreamEvent::Updates { node_id, state, .. } => {
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
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            ..
        } => {
            s.total_prompt_tokens = s.total_prompt_tokens.saturating_add(*prompt_tokens);
            s.total_completion_tokens = s.total_completion_tokens.saturating_add(*completion_tokens);
            tracing::info!(
                prompt_tokens,
                completion_tokens,
                total_tokens = *prompt_tokens + *completion_tokens,
                "LLM usage"
            );
        }
        _ => {}
    }
}

struct EventState {
    turn: u32,
    last_node: Option<String>,
    /// When output_timestamp is true, we print timestamp once before the first reply chunk.
    reply_started: bool,
    /// Agent name (source) to print before first reply chunk when set.
    agent_display: Option<String>,
    /// Accumulated prompt tokens from all StreamEvent::Usage in this run.
    total_prompt_tokens: u32,
    /// Accumulated completion tokens from all StreamEvent::Usage in this run.
    total_completion_tokens: u32,
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
    output_timestamp: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id, .. } => {
            log_node_enter(s.last_node.as_deref(), node_id, verbose);
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
            if !s.reply_started {
                if let Some(ref ad) = s.agent_display {
                    eprintln!("AGENT: {}", ad);
                }
                if output_timestamp {
                    print_reply_timestamp();
                }
                s.reply_started = true;
            }
            print_stream_chunk(chunk);
        }
        StreamEvent::Updates { node_id, state, .. } => {
            if verbose {
                eprintln!("--- state after {} ---", node_id);
                eprintln!("{}", format_got_state_display(state, display_max_len));
            }
        }
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            ..
        } => {
            s.total_prompt_tokens = s.total_prompt_tokens.saturating_add(*prompt_tokens);
            s.total_completion_tokens = s.total_completion_tokens.saturating_add(*completion_tokens);
            tracing::info!(
                prompt_tokens,
                completion_tokens,
                total_tokens = *prompt_tokens + *completion_tokens,
                "LLM usage"
            );
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
            github_token: None,
            mcp_github_cmd: "npx".to_string(),
            mcp_github_args: vec!["-y".to_string(), "@modelcontextprotocol/server-github".to_string()],
            mcp_github_url: None,
            mcp_verbose: false,
            openai_api_key: None,
            openai_base_url: None,
            model: None,
            llm_provider: None,
            embedding_api_key: None,
            embedding_base_url: None,
            embedding_model: None,
            working_folder: None,
            approval_policy: None,
            compaction_config: None,
            tot_config: TotRunnerConfig::default(),
            got_config: GotRunnerConfig::default(),
            mcp_servers: None,
            skill_registry: None,
            max_sub_agent_depth: None,
            dry_run: false,
        }
    }

    #[test]
    fn any_stream_event_to_format_a_and_protocol_format() {
        let ev = AnyStreamEvent::React(StreamEvent::TaskStart {
            node_id: "think".to_string(),
            namespace: None,
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
            reply_started: false,
            agent_display: None,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
        };
        on_event_react(
            &StreamEvent::TaskStart {
                node_id: "think".to_string(),
                namespace: None,
            },
            &mut s,
            100,
            true,
            false,
        );
        assert_eq!(s.last_node.as_deref(), Some("think"));

        on_event_react(
            &StreamEvent::Updates {
                node_id: "think".to_string(),
                state: react_state(),
                namespace: None,
            },
            &mut s,
            100,
            true,
            false,
        );
        assert_eq!(s.turn, 1);
    }

    #[test]
    fn on_event_dup_and_tot_and_got_do_not_panic() {
        let mut s = EventState {
            turn: 0,
            last_node: None,
            reply_started: false,
            agent_display: None,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
        };

        let dup_state = DupState {
            core: react_state(),
            understood: None,
        };
        on_event_dup(
            &StreamEvent::TaskStart {
                node_id: "understand".to_string(),
                namespace: None,
            },
            &mut s,
            120,
            true,
            false,
        );
        on_event_dup(
            &StreamEvent::Updates {
                node_id: "plan".to_string(),
                state: dup_state,
                namespace: None,
            },
            &mut s,
            120,
            true,
            false,
        );
        assert_eq!(s.turn, 1);

        let tot_state = TotState {
            core: react_state(),
            tot: TotExtension::default(),
        };
        on_event_tot(
            &StreamEvent::TaskStart {
                node_id: "think_expand".to_string(),
                namespace: None,
            },
            &mut s,
            120,
            true,
            false,
        );
        on_event_tot(
            &StreamEvent::TotExpand {
                candidates: vec!["a".to_string(), "b".to_string()],
            },
            &mut s,
            120,
            true,
            false,
        );
        on_event_tot(
            &StreamEvent::Updates {
                node_id: "observe".to_string(),
                state: tot_state,
                namespace: None,
            },
            &mut s,
            120,
            true,
            false,
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
                namespace: None,
            },
            &mut s,
            120,
            true,
            false,
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
            false,
        );
        on_event_got(
            &StreamEvent::Updates {
                node_id: "execute_graph".to_string(),
                state: got_state,
                namespace: None,
            },
            &mut s,
            120,
            true,
            false,
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
            reply_started: false,
            agent_display: None,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
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
                namespace: None,
            },
            &mut s,
            120,
            false,
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
                namespace: None,
            },
            &mut s,
            120,
            false,
            false,
        );
        assert_eq!(s.turn, 1);
    }

    #[test]
    fn verbose_tot_and_got_event_variants_are_handled() {
        let mut s = EventState {
            turn: 0,
            last_node: None,
            reply_started: false,
            agent_display: None,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
        };

        on_event_tot(
            &StreamEvent::TotEvaluate {
                chosen: 0,
                scores: vec![0.9],
            },
            &mut s,
            80,
            true,
            false,
        );
        on_event_tot(
            &StreamEvent::TotBacktrack {
                reason: "retry".to_string(),
                to_depth: 2,
            },
            &mut s,
            80,
            true,
            false,
        );
        on_event_tot(
            &StreamEvent::Messages {
                chunk: loom::MessageChunk::message("tok"),
                metadata: loom::StreamMetadata {
                    loom_node: "think_expand".to_string(),
                    namespace: None,
                },
            },
            &mut s,
            80,
            true,
            false,
        );

        on_event_got(
            &StreamEvent::GotNodeStart {
                node_id: "n1".to_string(),
            },
            &mut s,
            80,
            true,
            false,
        );
        on_event_got(
            &StreamEvent::GotNodeComplete {
                node_id: "n1".to_string(),
                result_summary: "done".to_string(),
            },
            &mut s,
            80,
            true,
            false,
        );
        on_event_got(
            &StreamEvent::GotNodeFailed {
                node_id: "n2".to_string(),
                error: "boom".to_string(),
            },
            &mut s,
            80,
            true,
            false,
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
            false,
        );
        on_event_got(
            &StreamEvent::Messages {
                chunk: loom::MessageChunk::message("chunk"),
                metadata: loom::StreamMetadata {
                    loom_node: "execute_graph".to_string(),
                    namespace: None,
                },
            },
            &mut s,
            80,
            true,
            false,
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
            session_id: None,
            cancellation: None,
            thread_id: None,
            role_file: None,
            agent: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 200,
            output_json,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
            provider: None,
            base_url: None,
            api_key: None,
            provider_type: None,
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
