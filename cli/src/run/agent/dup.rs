use crate::run::display::{format_dup_state_display, truncate_display};
use loom::{DupState, StreamEvent};

use super::common::{handle_messages, usage_simple};
use super::{log_node_enter, log_tools_used, EventState};

pub(crate) fn on_event_dup(
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
                    log_tools_used(&state.core.tool_calls);
                }
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
