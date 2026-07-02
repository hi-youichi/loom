use loom_stream::{StubDupState, StubGotState, StubTotState};
use agent::state::{ReActState, ToolCall, ToolResult};
use loom_llm::message::Message;

const INDENT: &str = "  ";

pub fn truncate_display(s: &str, max: usize) -> String {
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
        SUFFIX,
    )
}

pub fn format_message_truncated(m: &Message, max: usize) -> String {
    match m {
        Message::System(s) => format!("System({})", truncate_display(s, max)),
        Message::User(s) => format!("User({})", truncate_display(s.as_text().as_ref(), max)),
        Message::Assistant(p) => {
            format!("Assistant({})", truncate_display(p.content.as_str(), max))
        }
        Message::Tool {
            tool_call_id,
            content,
        } => format!(
            "Tool({}:{})",
            tool_call_id,
            truncate_display(&content.to_display_string(), max)
        ),
    }
}

pub fn format_tool_call_truncated(tc: &ToolCall, max: usize) -> String {
    format!(
        "ToolCall {{ name: {:?}, arguments: {:?}, id: {:?} }}",
        tc.name,
        truncate_display(&tc.arguments, max),
        tc.id
    )
}

pub fn format_tool_result_truncated(tr: &ToolResult, max: usize) -> String {
    format!(
        "ToolResult {{ call_id: {:?}, name: {:?}, content: {:?} }}",
        tr.call_id,
        tr.name,
        truncate_display(&tr.content, max)
    )
}

pub fn format_react_state_display(state: &ReActState, max: usize) -> String {
    let mut lines = vec!["ReActState {".to_string()];

    lines.push(format!("{}messages:", INDENT));
    for m in &state.messages {
        lines.push(format!(
            "{}{}{}",
            INDENT,
            INDENT,
            format_message_truncated(m, max)
        ));
    }

    lines.push(format!("{}tool_calls:", INDENT));
    for tc in &state.tool_calls {
        lines.push(format!(
            "{}{}{}",
            INDENT,
            INDENT,
            format_tool_call_truncated(tc, max)
        ));
    }

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
    lines.push("}".to_string());

    lines.join("\n")
}

pub fn indent_lines(s: &str, indent: &str) -> String {
    s.lines()
        .map(|line| format!("{}{}", indent, line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_tot_state_display(state: &StubTotState, max: usize) -> String {
    let core_block = format_react_state_display(&state.core, max);
    let core_indented = indent_lines(&core_block, "    ");
    let lines = [
        "TotState {".to_string(),
        format!("{}core:", INDENT),
        core_indented,
        "}".to_string(),
    ];
    lines.join("\n")
}

pub fn format_dup_state_display(state: &StubDupState, max: usize) -> String {
    let core_block = format_react_state_display(&state.core, max);
    let core_indented = indent_lines(&core_block, "    ");
    let lines = [
        "DupState {".to_string(),
        format!("{}core:", INDENT),
        core_indented,
        "}".to_string(),
    ];
    lines.join("\n")
}

pub fn format_got_state_display(state: &StubGotState, max: usize) -> String {
    format!(
        "GotState {{ input_message: {} }}",
        truncate_display(&state.input_message, max),
    )
}

pub fn format_context_limit(limit: u32) -> String {
    if limit >= 1_000_000 {
        format!("{:.1}M", limit as f64 / 1_000_000.0)
    } else if limit >= 1000 {
        format!("{}K", limit / 1000)
    } else {
        limit.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_stream::{StubDupState, StubTotState};

    // Note: Tests for format_tot_state_display, format_dup_state_display, format_got_state_display
    // are moved to loom-agent crate since they use agent-specific types.

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
            format_message_truncated(&Message::assistant("hello assistant"), 8),
            "Assistant(hello...)"
        );
    }

    #[test]
    fn format_react_state_display_contains_sections() {
        let state = ReActState {
            messages: vec![Message::user("question"), Message::assistant("answer")],
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
                ..ToolResult::default()
            }],
            turn_count: 2,
            ..ReActState::default()
        };

        let rendered = format_react_state_display(&state, 12);
        assert!(rendered.contains("ReActState {"));
        assert!(rendered.contains("messages:"));
        assert!(rendered.contains("tool_calls:"));
        assert!(rendered.contains("tool_results:"));
        assert!(rendered.contains("turn_count: 2"));
        assert!(rendered.contains("Assistant(answer)"));
        assert!(rendered.contains("..."));
    }

    #[test]
    fn format_tot_and_dup_state_embed_core_block() {
        let core = ReActState {
            messages: vec![Message::user("u"), Message::assistant("a")],
            ..ReActState::default()
        };
        let tot = StubTotState {
            core: core.clone(),
        };
        let dup = StubDupState {
            core,
        };

        let tot_rendered = format_tot_state_display(&tot, 20);
        assert!(tot_rendered.contains("TotState"));
        assert!(tot_rendered.contains("core:"));
        assert!(tot_rendered.contains("ReActState {"));

        let dup_rendered = format_dup_state_display(&dup, 20);
        assert!(dup_rendered.contains("DupState"));
        assert!(dup_rendered.contains("core:"));
        assert!(dup_rendered.contains("ReActState {"));
    }

    // Note: format_got_state_display test moved to loom-agent crate (uses TaskGraph, TaskNodeState)
}