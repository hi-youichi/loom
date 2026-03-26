use crate::run::display::format_got_state_display;
use loom::{GotState, StreamEvent};

use super::common::{handle_messages, usage_simple};
use super::{log_node_enter, EventState};

pub(crate) fn on_event_got(
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
            handle_messages(s, chunk, output_timestamp);
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
        } => usage_simple(s, *prompt_tokens, *completion_tokens),
        _ => {}
    }
}
