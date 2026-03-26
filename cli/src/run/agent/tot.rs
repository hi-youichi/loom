use crate::run::display::format_tot_state_display;
use loom::{StreamEvent, TotState};

use super::common::{handle_messages, usage_simple};
use super::{log_node_enter, log_tools_used, EventState};

pub(crate) fn on_event_tot(
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
                log_tools_used(&state.core.tool_calls);
            }
        }
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            ..
        } => usage_simple(s, *prompt_tokens, *completion_tokens),
        _ => {}
    }
}
