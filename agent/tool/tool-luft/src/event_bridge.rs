use agent::agent::AgentEvent as LoomAgentEvent;
use luft_core::contract::event::{AgentEvent as LuftAgentEvent, ProgressDelta};

pub fn map_loom_event_to_delta(ev: &LoomAgentEvent) -> Option<ProgressDelta> {
    match ev {
        LoomAgentEvent::TextChunk(text) => Some(ProgressDelta::Message {
            text: text.clone(),
        }),
        LoomAgentEvent::ToolCallStart { name, arguments } => {
            let summary = if arguments.len() > 200 {
                format!("{}({}...)", name, &arguments[..200])
            } else {
                format!("{}({})", name, arguments)
            };
            Some(ProgressDelta::ToolCall {
                name: name.clone(),
                summary,
            })
        }
        LoomAgentEvent::ToolEnd { name, is_error, .. } => Some(ProgressDelta::ToolCall {
            name: name.clone(),
            summary: if *is_error { "error".to_string() } else { "done".to_string() },
        }),
        LoomAgentEvent::ReasoningChunk(_) => None,
        LoomAgentEvent::ToolOutput { .. } => None,
        LoomAgentEvent::Usage { .. } => None,
    }
}

pub fn luft_event_to_json(ev: &LuftAgentEvent) -> serde_json::Value {
    serde_json::to_value(ev).unwrap_or(serde_json::json!({"type": "unknown"}))
}
