//! Message and state display formatting for stderr output.
//!
//! Provides truncation and formatting utilities for [`Message`](loom::Message)
//! and agent state types (ReActState, TotState, DupState, GotState) when streaming
//! to the CLI.

use loom::{DupState, GotState, Message, ReActState, ToolCall, ToolResult, TotState};
use std::collections::HashMap;

/// Indent for nested state fields (one level).
const INDENT: &str = "  ";

/// Truncates a string to at most `max` chars; appends "..." when truncated. UTF-8 safe.
pub(crate) fn truncate_display(s: &str, max: usize) -> String {
    const SUFFIX: &str = "...";
    let suffix_len = 3;
    if max <= suffix_len {
        return s.chars().take(max).collect();
    }
    let content_max = max - suffix_len;
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!(
        "{}{}",
        s.chars().take(content_max).collect::<String>(),
        SUFFIX
    )
}

/// Formats one Message with content truncated for display (User/Assistant/System).
pub(crate) fn format_message_truncated(m: &Message, max: usize) -> String {
    match m {
        Message::System(s) => format!("System({})", truncate_display(s, max)),
        Message::User(s) => format!("User({})", truncate_display(s, max)),
        Message::Assistant(s) => format!("Assistant({})", truncate_display(s, max)),
    }
}

/// Formats one ToolCall with arguments truncated for display.
fn format_tool_call_truncated(tc: &ToolCall, max: usize) -> String {
    format!(
        "ToolCall {{ name: {:?}, arguments: {:?}, id: {:?} }}",
        tc.name,
        truncate_display(&tc.arguments, max),
        tc.id
    )
}

/// Formats one ToolResult with content truncated for display.
fn format_tool_result_truncated(tr: &ToolResult, max: usize) -> String {
    format!(
        "ToolResult {{ call_id: {:?}, name: {:?}, content: {:?} }}",
        tr.call_id,
        tr.name,
        truncate_display(&tr.content, max)
    )
}

/// Formats ReActState for stderr: one field per line, one message/tool_call/tool_result per line.
pub(crate) fn format_react_state_display(state: &ReActState, max: usize) -> String {
    let mut lines = vec!["ReActState {".to_string()];

    // messages: one per line
    lines.push(format!("{}messages:", INDENT));
    for m in &state.messages {
        lines.push(format!(
            "{}{}{}",
            INDENT,
            INDENT,
            format_message_truncated(m, max)
        ));
    }

    // tool_calls: one per line
    lines.push(format!("{}tool_calls:", INDENT));
    for tc in &state.tool_calls {
        lines.push(format!(
            "{}{}{}",
            INDENT,
            INDENT,
            format_tool_call_truncated(tc, max)
        ));
    }

    // tool_results: one per line
    lines.push(format!("{}tool_results:", INDENT));
    for tr in &state.tool_results {
        lines.push(format!(
            "{}{}{}",
            INDENT,
            INDENT,
            format_tool_result_truncated(tr, max)
        ));
    }

    lines.push(format!("{}turn_count: {}", INDENT, state.turn_count));
    lines.push(format!(
        "{}approval_result: {:?}",
        INDENT, state.approval_result
    ));
    lines.push("}".to_string());

    lines.join("\n")
}

/// Prefix each line of `s` with `indent` (for embedding multi-line state in outer state).
fn indent_lines(s: &str, indent: &str) -> String {
    s.lines()
        .map(|line| format!("{}{}", indent, line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Formats TotState for stderr (core multi-line, tot on its own line).
pub(crate) fn format_tot_state_display(state: &TotState, max: usize) -> String {
    let core_block = format_react_state_display(&state.core, max);
    let core_indented = indent_lines(&core_block, "    ");
    let lines = vec![
        "TotState {".to_string(),
        format!("{}core:", INDENT),
        core_indented,
        format!("{}tot: {:?}", INDENT, state.tot),
        "}".to_string(),
    ];
    lines.join("\n")
}

/// Formats DupState for stderr (core multi-line, understood on its own line).
pub(crate) fn format_dup_state_display(state: &DupState, max: usize) -> String {
    let core_block = format_react_state_display(&state.core, max);
    let core_indented = indent_lines(&core_block, "    ");
    let lines = vec![
        "DupState {".to_string(),
        format!("{}core:", INDENT),
        core_indented,
        format!("{}understood: {:?}", INDENT, state.understood),
        "}".to_string(),
    ];
    lines.join("\n")
}

/// Formats GotState for stderr (input_message and node result/error truncated).
pub(crate) fn format_got_state_display(state: &GotState, max: usize) -> String {
    let node_states: HashMap<String, String> = state
        .node_states
        .iter()
        .map(|(id, ns)| {
            let r = ns
                .result
                .as_ref()
                .map(|s| truncate_display(s, max))
                .unwrap_or_else(|| "None".to_string());
            let e = ns
                .error
                .as_ref()
                .map(|s| truncate_display(s, max))
                .unwrap_or_else(|| "None".to_string());
            (
                id.clone(),
                format!(
                    "TaskNodeState {{ status: {:?}, result: {:?}, error: {:?} }}",
                    ns.status, r, e
                ),
            )
        })
        .collect();
    format!(
        "GotState {{ input_message: {}, task_graph: {:?}, node_states: {:?} }}",
        truncate_display(&state.input_message, max),
        state.task_graph,
        node_states
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom::{TaskGraph, TaskNode, TaskNodeState, TaskStatus, ToolCall, ToolResult};
    use std::collections::HashMap;

    #[test]
    fn truncate_display_handles_short_exact_and_truncated() {
        assert_eq!(truncate_display("abc", 10), "abc");
        assert_eq!(truncate_display("abcdef", 3), "abc");
        assert_eq!(truncate_display("abcdefghij", 5), "ab...");
    }

    #[test]
    fn format_message_truncated_for_all_variants() {
        assert_eq!(
            format_message_truncated(&Message::System("hello system".into()), 8),
            "System(hello...)"
        );
        assert_eq!(
            format_message_truncated(&Message::User("hello user".into()), 8),
            "User(hello...)"
        );
        assert_eq!(
            format_message_truncated(&Message::Assistant("hello assistant".into()), 8),
            "Assistant(hello...)"
        );
    }

    #[test]
    fn format_react_state_display_contains_sections() {
        let state = ReActState {
            messages: vec![
                Message::user("question"),
                Message::Assistant("answer".into()),
            ],
            tool_calls: vec![ToolCall {
                name: "web_fetch".to_string(),
                arguments: r#"{"url":"https://example.com/very/long/path"}"#.to_string(),
                id: Some("c1".to_string()),
            }],
            tool_results: vec![ToolResult {
                call_id: Some("c1".to_string()),
                name: Some("web_fetch".to_string()),
                content: "very long tool content output".to_string(),
                is_error: false,
            }],
            turn_count: 2,
            approval_result: Some(true),
            ..ReActState::default()
        };

        let rendered = format_react_state_display(&state, 12);
        assert!(rendered.contains("ReActState {"));
        assert!(rendered.contains("messages:"));
        assert!(rendered.contains("tool_calls:"));
        assert!(rendered.contains("tool_results:"));
        assert!(rendered.contains("turn_count: 2"));
        assert!(rendered.contains("approval_result: Some(true)"));
        assert!(rendered.contains("Assistant(answer)"));
        assert!(rendered.contains("..."));
    }

    #[test]
    fn format_tot_and_dup_state_embed_core_block() {
        let core = ReActState {
            messages: vec![Message::user("u"), Message::Assistant("a".into())],
            ..ReActState::default()
        };
        let tot = TotState {
            core: core.clone(),
            tot: loom::TotExtension::default(),
        };
        let dup = DupState {
            core,
            understood: None,
        };

        let tot_rendered = format_tot_state_display(&tot, 20);
        assert!(tot_rendered.contains("TotState {"));
        assert!(tot_rendered.contains("core:"));
        assert!(tot_rendered.contains("tot:"));
        assert!(tot_rendered.contains("ReActState {"));

        let dup_rendered = format_dup_state_display(&dup, 20);
        assert!(dup_rendered.contains("DupState {"));
        assert!(dup_rendered.contains("core:"));
        assert!(dup_rendered.contains("understood:"));
        assert!(dup_rendered.contains("ReActState {"));
    }

    #[test]
    fn format_got_state_display_truncates_input_and_node_state() {
        let mut node_states = HashMap::new();
        node_states.insert(
            "n1".to_string(),
            TaskNodeState {
                status: TaskStatus::Done,
                result: Some("this is a very long result text".to_string()),
                error: None,
            },
        );
        let state = GotState {
            input_message: "this is a very long input message".to_string(),
            task_graph: TaskGraph {
                nodes: vec![TaskNode {
                    id: "n1".to_string(),
                    description: "desc".to_string(),
                    tool_calls: vec![],
                }],
                edges: vec![],
            },
            node_states,
        };

        let rendered = format_got_state_display(&state, 12);
        assert!(rendered.contains("GotState {"));
        assert!(rendered.contains("task_graph"));
        assert!(rendered.contains("node_states"));
        assert!(rendered.contains("TaskNodeState"));
        assert!(rendered.contains("..."));
    }
}
