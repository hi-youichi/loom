use crate::state::ReActState;
use std::time::Instant;
use stream_event::StreamEvent;

pub struct SubagentDisplay {
    agent_name: String,
    depth: usize,
    indent_size: usize,
}

impl SubagentDisplay {
    pub fn new(agent_name: String, depth: u32) -> Self {
        Self {
            agent_name,
            depth: depth as usize,
            indent_size: 2,
        }
    }

    fn indent(&self) -> String {
        " ".repeat(self.depth * self.indent_size)
    }

    fn prefix(&self) -> String {
        format!("{}↳ [{}]", self.indent(), self.agent_name)
    }

    pub fn format_event(
        &self,
        event: &StreamEvent<ReActState>,
        start_time: Instant,
    ) -> Option<String> {
        match event {
            StreamEvent::TaskStart { .. } => Some(format!("{} Starting...", self.prefix())),

            StreamEvent::TaskEnd { result, .. } => {
                let elapsed = start_time.elapsed();
                let status = if result.is_ok() { "done" } else { "failed" };
                Some(format!(
                    "{} {} ({:.1}s)",
                    self.prefix(),
                    status,
                    elapsed.as_secs_f64()
                ))
            }

            StreamEvent::TextDelta { content, .. } => self.format_message_text(content),

            StreamEvent::ReasoningDelta { content, .. } => {
                self.format_message_reasoning(content)
            }

            StreamEvent::Updates { node_id, state, .. } => self.format_updates(node_id, state),

            StreamEvent::ToolStart { name, .. } => {
                Some(format!("{} Running: {}", self.prefix(), name))
            }

            StreamEvent::ToolEnd { name, is_error, .. } => {
                let icon = if *is_error { "✗" } else { "✓" };
                Some(format!("{} {} {}", self.prefix(), icon, name))
            }

            _ => None,
        }
    }

    fn format_message_text(&self, content: &str) -> Option<String> {
        if content.trim().is_empty() {
            return None;
        }
        let text = truncate(&content.replace('\n', " "), 100);
        Some(format!("{} {}", self.prefix(), text))
    }

    fn format_message_reasoning(&self, content: &str) -> Option<String> {
        if content.trim().is_empty() {
            return None;
        }
        let text = truncate(content, 50);
        Some(format!("{} {}", self.prefix(), dim(&text)))
    }

    fn format_updates(&self, node_id: &str, state: &ReActState) -> Option<String> {
        let node_short = node_id.split('/').next_back().unwrap_or(node_id);
        match node_short {
            "think" => {
                if let Some(thought) = &state.last_reasoning_content {
                    let text = truncate(thought, 80);
                    Some(format!("{} Thinking: {}", self.prefix(), text))
                } else {
                    None
                }
            }
            "act" => {
                if !state.tool_calls.is_empty() {
                    let tools: Vec<&str> =
                        state.tool_calls.iter().map(|tc| tc.name.as_str()).collect();
                    Some(format!("{} Calling: {}", self.prefix(), tools.join(", ")))
                } else {
                    None
                }
            }
            "observe" => {
                if !state.tool_results.is_empty() {
                    Some(format!(
                        "{} {} tool(s) completed",
                        self.prefix(),
                        state.tool_results.len()
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

fn dim(s: &str) -> String {
    format!("\x1b[2m{}\x1b[0m", s)
}

pub fn format_subagent_event(
    event: &StreamEvent<ReActState>,
    agent_name: &str,
    depth: u32,
    start_time: Instant,
) -> Option<String> {
    let display = SubagentDisplay::new(agent_name.to_string(), depth);
    display.format_event(event, start_time)
}
