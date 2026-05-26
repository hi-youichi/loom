use chrono::Local;
use std::sync::Mutex;

use crate::cli_run::AnyStreamEvent;
use crate::stream::{MessageChunk, MessageChunkKind, StreamEvent};
use crate::{DupState, GotState, ReActState, ToolCall, ToolResult, TotState};
use crate::stream_display::format::*;
use crate::stream_display::panel_format;
use crate::stream_display::spinner::{NoopSpinner, Spinner, SpinnerTrait};

pub struct StreamDisplayConfig {
    pub verbose: bool,
    pub display_max_len: usize,
    pub output_timestamp: bool,
    pub agent_display: Option<String>,
    /// Whether to show an animated spinner while waiting for LLM responses.
    pub use_spinner: bool,
}

pub struct EventState {
    pub turn: u32,
    pub last_node: Option<String>,
    pub reply_started: bool,
    pub agent_display: Option<String>,
    pub total_prompt_tokens: u32,
    pub total_completion_tokens: u32,
    pub in_thinking: bool,
    pub last_prefill_duration: Option<std::time::Duration>,
    pub last_decode_duration: Option<std::time::Duration>,
    pub pending_tool_calls: Vec<ToolCall>,
    /// Time when pending_tool_calls were received (for elapsed timing).
    pub pending_tool_start: Option<std::time::Instant>,
    /// Tool results from the act node (saved before observe clears them).
    pub pending_tool_results: Vec<ToolResult>,
    /// Active spinner (if any). Created on TaskStart, finished when streaming begins.
    pub spinner: Option<Box<dyn SpinnerTrait>>,
    /// Whether to use animated spinners.
    pub use_spinner: bool,
    /// Whether to use compact mode (hide PREVIEW/DIFF).
    pub compact: bool,
    /// When the overall session started, for "共 Ns" elapsed display.
    pub session_start: Option<std::time::Instant>,
}

impl EventState {
    pub fn new(agent_display: Option<String>, use_spinner: bool) -> Self {
        Self {
            turn: 0,
            last_node: None,
            reply_started: false,
            agent_display,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
            in_thinking: false,
            last_prefill_duration: None,
            last_decode_duration: None,
            pending_tool_calls: Vec::new(),
            pending_tool_start: None,
            pending_tool_results: Vec::new(),
            spinner: None,
            use_spinner,
            compact: false,
            session_start: None,
        }
    }

    /// Create a new spinner (animated or noop depending on config).
    fn create_spinner(&self, label: String) -> Box<dyn SpinnerTrait> {
        if self.use_spinner {
            Box::new(Spinner::new(label))
        } else {
            Box::new(NoopSpinner::new(label))
        }
    }
}

pub fn create_stdio_event_callback(
    config: StreamDisplayConfig,
) -> Box<dyn FnMut(AnyStreamEvent) + Send> {
    let state = Mutex::new(EventState::new(config.agent_display, config.use_spinner));
    let display_max_len = config.display_max_len;
    let verbose = config.verbose;
    let output_timestamp = config.output_timestamp;

    Box::new(move |ev: AnyStreamEvent| {
        let mut s = match state.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        match &ev {
            AnyStreamEvent::React(e) => {
                on_event_react(e, &mut s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Dup(e) => {
                on_event_dup(e, &mut s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Tot(e) => {
                on_event_tot(e, &mut s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Got(e) => {
                on_event_got(e, &mut s, display_max_len, verbose, output_timestamp)
            }
        }
    })
}

pub fn print_reply_timestamp() {
    eprintln!("{}", Local::now().format("%Y-%m-%d %H:%M:%S"));
}

pub fn log_node_enter(from: Option<&str>, node_id: &str, verbose: bool) {
    if !verbose {
        return;
    }
    let from = from.unwrap_or("START");
    eprintln!("Entering: {} (from {})", node_id, from);
}

pub fn log_tools_used(tool_calls: &[ToolCall]) {
    if tool_calls.is_empty() {
        return;
    }
    for tc in tool_calls {
        let summary = crate::stream_display::tool_summary::format_call_summary(&tc.name, &tc.arguments);
        eprintln!("{}", panel_format::format_tool_call(&tc.name, &summary));
    }
}

pub fn print_stream_chunk(chunk: &MessageChunk) {
    if chunk.kind == MessageChunkKind::Thinking {
        eprint!("{}", panel_format::dim(&chunk.content));
        let _ = std::io::Write::flush(&mut std::io::stderr());
    } else {
        print!("{}", chunk.content);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

fn handle_messages(s: &mut EventState, chunk: &MessageChunk, output_timestamp: bool) {
    if chunk.kind == MessageChunkKind::Thinking {
        s.in_thinking = true;
    }
    if let Some(sp) = s.spinner.take() {
        sp.finish_box();
    }
    if !s.reply_started {
        if let Some(ref ad) = s.agent_display {
            eprintln!("{}", panel_format::format_panel_line("AGENT", ad));
        }
        if output_timestamp {
            print_reply_timestamp();
        }
        s.reply_started = true;
    }
    if s.in_thinking && chunk.kind != MessageChunkKind::Thinking {
                eprintln!();
                eprintln!("{}", panel_format::format_thinking_separator());
        s.in_thinking = false;
    }
    print_stream_chunk(chunk);
}

fn handle_usage(
    s: &mut EventState,
    prompt_tokens: &u32,
    completion_tokens: &u32,
    prefill_duration: &Option<std::time::Duration>,
    decode_duration: &Option<std::time::Duration>,
    verbose: bool,
) {
    s.total_prompt_tokens = s.total_prompt_tokens.saturating_add(*prompt_tokens);
    s.total_completion_tokens = s.total_completion_tokens.saturating_add(*completion_tokens);
    s.last_prefill_duration = *prefill_duration;
    s.last_decode_duration = *decode_duration;

    let total_dur = match (prefill_duration, decode_duration) {
        (Some(pf), Some(dc)) => *pf + *dc,
        _ => std::time::Duration::ZERO,
    };
    eprintln!(
        "\n{}",
        panel_format::format_usage_line(
            total_dur,
            *prompt_tokens,
            *completion_tokens,
            *prefill_duration,
            *decode_duration,
            verbose,
        )
    );

    tracing::info!(
        prompt_tokens,
        completion_tokens,
        total_tokens = *prompt_tokens + *completion_tokens,
        "LLM usage"
    );
}

pub fn on_event_react(
    ev: &StreamEvent<ReActState>,
    s: &mut EventState,
    display_max_len: usize,
    verbose: bool,
    output_timestamp: bool,
) {
    match ev {
        StreamEvent::TaskStart { node_id, .. } => {
            if let Some(sp) = s.spinner.take() {
                sp.finish_box();
                eprintln!();
            }
            if node_id == "think" {
                // Initialize session_start on first think
                if s.session_start.is_none() {
                    s.session_start = Some(std::time::Instant::now());
                }
                let turn = s.turn + 1;
                let session_start = s.session_start.unwrap();
                let sp: Box<dyn SpinnerTrait> = if s.use_spinner {
                    let sp = Spinner::new("思考中...".to_string());
                    sp.set_context(turn, session_start);
                    Box::new(sp)
                } else {
                    Box::new(NoopSpinner::new("思考中...".to_string()))
                };
                s.spinner = Some(sp);
            }
            log_node_enter(s.last_node.as_deref(), node_id, verbose);
            s.last_node = Some(node_id.clone());
        }
        StreamEvent::Messages { chunk, .. } => {
            handle_messages(s, chunk, output_timestamp);
        }
        StreamEvent::Updates { node_id, state, .. } => {
            // Always show title generation result (non-verbose too)
            if node_id == "title" {
                if let Some(ref title) = state.summary {
                    eprintln!("Session title: {}", title);
                }
            }
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
            } else {
                // Save tool_calls during think (non-verbose)
                if node_id == "think" && !state.tool_calls.is_empty() {
                    if let Some(sp) = s.spinner.take() {
                        sp.finish_box();
                    }
                    log_tools_used(&state.tool_calls);
                    if let Some(tc) = state.tool_calls.first() {
                        s.spinner = Some(s.create_spinner(format!("执行工具: {}", tc.name)));
                    }
                    s.pending_tool_calls = state.tool_calls.clone();
                    s.pending_tool_start = Some(std::time::Instant::now());
                }
                // Save tool_results during act (observe will clear them)
                if node_id == "act" && !state.tool_results.is_empty() {
                    s.pending_tool_results = state.tool_results.clone();
                }
            }
            if node_id == "observe" {
                let elapsed = s.pending_tool_start.map(|t| t.elapsed());
                let compact = s.compact;
                // Use cached tool results from act (observe clears tool_results)
                let tool_results = if state.tool_results.is_empty() {
                    &s.pending_tool_results
                } else {
                    &state.tool_results
                };
                for tc in s.pending_tool_calls.drain(..) {
                    let result_text = find_tool_result(tool_results, &tc.name, &tc.id);
                    let is_error = find_tool_result_error(tool_results, &tc.name, &tc.id);

                    if is_error {
                        let err_msg = match &result_text {
                            Some(r) => r.lines().next().unwrap_or("error"),
                            None => "error",
                        };
                        eprintln!("{}", panel_format::format_panel_line("ERROR", &format!("{}: {}", tc.name, crate::stream_display::tool_summary::truncate(err_msg, 80))));
                    }

                    if let Some(ref result) = result_text {
                        if let Some(preview) = crate::stream_display::tool_preview::format_preview(
                            &tc.name, &tc.arguments, result, compact,
                        ) {
                            eprintln!("{}", preview);
                        } else if !is_error && !result.trim().is_empty() && !compact {
                            eprintln!("{}", crate::stream_display::tool_preview::format_result_preview(
                                &tc.name, result, elapsed,
                            ));
                        }
                    }

                    if let Some(ref result) = result_text {
                        if let Some(diff) = crate::stream_display::tool_preview::format_diff(
                            &tc.name, &tc.arguments, result, compact,
                        ) {
                            eprintln!("{}", diff);
                        }
                    }

                    // Print DONE line for each completed tool
                    let result_summary = result_text.as_deref().unwrap_or("");
                    eprintln!("{}", panel_format::format_tool_done(&tc.name, result_summary, elapsed));
                }
                s.pending_tool_start = None;
                s.pending_tool_results.clear();
            }
        }
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            prefill_duration,
            decode_duration,
            ..
        } => {
            handle_usage(s, prompt_tokens, completion_tokens, prefill_duration, decode_duration, verbose);
        }
        _ => {}
    }
}

pub fn on_event_dup(
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
            handle_messages(s, chunk, output_timestamp);
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
            } else if node_id == "plan" {
                s.turn += 1;
                if !state.core.tool_calls.is_empty() {
                    if let Some(sp) = s.spinner.take() {
                        sp.finish_box();
                    }
                    if let Some(tc) = state.core.tool_calls.first() {
                        s.spinner = Some(s.create_spinner(format!("Executing tool: {}", tc.name)));
                    }
                }
            }
        }
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            prefill_duration,
            decode_duration,
            ..
        } => {
            handle_usage(s, prompt_tokens, completion_tokens, prefill_duration, decode_duration, verbose);
        }
        _ => {}
    }
}

pub fn on_event_tot(
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
        StreamEvent::TotExpand { candidates } if verbose => {
            eprintln!("--- ToT expand: {} candidates ---", candidates.len());
            for (i, c) in candidates.iter().enumerate() {
                eprintln!("  [{}] {}", i + 1, c);
            }
        }
        StreamEvent::TotEvaluate { chosen, scores } if verbose => {
            eprintln!(
                "--- ToT evaluate: chosen={}, scores={:?} ---",
                chosen, scores
            );
        }
        StreamEvent::TotBacktrack { reason, to_depth } if verbose => {
            eprintln!(
                "--- ToT backtrack: reason={}, to_depth={} ---",
                reason, to_depth
            );
        }
        StreamEvent::Messages { chunk, .. } => {
            handle_messages(s, chunk, output_timestamp);
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
            }
        }
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            prefill_duration,
            decode_duration,
            ..
        } => {
            handle_usage(s, prompt_tokens, completion_tokens, prefill_duration, decode_duration, verbose);
        }
        _ => {}
    }
}

pub fn on_event_got(
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
        } if verbose => {
            eprintln!(
                "--- GoT plan: {} nodes, {} edges ---",
                node_count, edge_count
            );
            for id in node_ids {
                eprintln!("  node: {}", id);
            }
        }
        StreamEvent::GotNodeStart { node_id } if verbose => {
            eprintln!("--- GoT node start: {} ---", node_id);
        }
        StreamEvent::GotNodeComplete {
            node_id,
            result_summary,
        } if verbose => {
            eprintln!("--- GoT node complete: {} ---", node_id);
            eprintln!("  result: {}", result_summary);
        }
        StreamEvent::GotNodeFailed { node_id, error } if verbose => {
            eprintln!("--- GoT node failed: {} ---", node_id);
            eprintln!("  error: {}", error);
        }
        StreamEvent::GotExpand {
            node_id,
            nodes_added,
            edges_added,
        } if verbose => {
            eprintln!(
                "--- AGoT expand: {} → +{} nodes, +{} edges ---",
                node_id, nodes_added, edges_added
            );
        }
        StreamEvent::Messages { chunk, .. } => {
            handle_messages(s, chunk, output_timestamp);
        }
        StreamEvent::Updates { node_id, state, .. } if verbose => {
            eprintln!("--- state after {} ---", node_id);
            eprintln!("{}", format_got_state_display(state, display_max_len));
        }
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            prefill_duration,
            decode_duration,
            ..
        } => {
            handle_usage(s, prompt_tokens, completion_tokens, prefill_duration, decode_duration, verbose);
        }
        _ => {}
    }
}


// ── Tool result helpers ──────────────────────────────────────────

/// Find the tool result text matching a tool call by name and/or id.
pub fn find_tool_result(results: &[ToolResult], tool_name: &str, call_id: &Option<String>) -> Option<String> {
    // Try matching by id first
    if let Some(ref id) = call_id {
        if let Some(tr) = results.iter().find(|r| r.call_id.as_deref() == Some(id.as_str())) {
            return Some(tr.observation_text.clone().unwrap_or_else(|| tr.content.clone()));
        }
    }
    // Fallback: match by name
    if let Some(tr) = results.iter().find(|r| r.name.as_deref() == Some(tool_name)) {
        return Some(tr.observation_text.clone().unwrap_or_else(|| tr.content.clone()));
    }
    // Last resort: return first result if only one
    if results.len() == 1 {
        let tr = &results[0];
        return Some(tr.observation_text.clone().unwrap_or_else(|| tr.content.clone()));
    }
    None
}

/// Check if a tool result is an error.
pub fn find_tool_result_error(results: &[ToolResult], tool_name: &str, call_id: &Option<String>) -> bool {
    if let Some(ref id) = call_id {
        if let Some(tr) = results.iter().find(|r| r.call_id.as_deref() == Some(id.as_str())) {
            return tr.is_error;
        }
    }
    if let Some(tr) = results.iter().find(|r| r.name.as_deref() == Some(tool_name)) {
        return tr.is_error;
    }
    false
}