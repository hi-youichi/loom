use crate::run::display::format_react_state_display;
use loom::{ReActState, StreamEvent};

use super::common::{handle_messages, usage_react};
use super::{log_node_enter, log_tools_used, EventState};

pub(crate) fn on_event_react(
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
            handle_messages(s, chunk, output_timestamp);
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
        } => usage_react(
            s,
            *prompt_tokens,
            *completion_tokens,
            *prefill_duration,
            *decode_duration,
        ),
        _ => {}
    }
}
