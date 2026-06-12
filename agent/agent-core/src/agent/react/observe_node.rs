//! Observe node: read tool_results, merge into state (e.g. messages), clear tool_calls and tool_results.

use async_trait::async_trait;
use tracing::{debug, warn};

use loom_llm::error::AgentError;
use loom_graph::Next;
use loom_memory::uuid6;
use loom_llm::message::{message_summary, Message};
use loom_cli_types::ReActState;
use tool_core::ToolCallContent;
use loom_graph::Node;

pub struct ObserveNode {
    enable_loop: bool,
    /// When `Some(n)`, end loop after n observe rounds. When `None` (default for with_loop), no limit.
    max_turns: Option<u32>,
}

impl ObserveNode {
    pub fn new() -> Self {
        Self {
            enable_loop: false,
            max_turns: None,
        }
    }

    /// ReAct loop: observe can continue back to think. No turn limit by default.
    pub fn with_loop() -> Self {
        Self {
            enable_loop: true,
            max_turns: None,
        }
    }

    /// ReAct loop with a maximum number of observe rounds; after this, exit with max_turns_reached.
    pub fn with_loop_max_turns(max_turns: u32) -> Self {
        Self {
            enable_loop: true,
            max_turns: Some(max_turns),
        }
    }
}

impl Default for ObserveNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Node<ReActState> for ObserveNode {
    fn id(&self) -> &str {
        "observe"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let had_tool_calls = !state.tool_calls.is_empty();
        let messages_before = state.messages.len();
        debug!(
            had_tool_calls,
            tool_results = state.tool_results.len(),
            messages_before,
            "observe:input"
        );
        let mut messages = state.messages;
        for tr in &state.tool_results {
            let name = tr
                .name
                .as_deref()
                .or(tr.call_id.as_deref())
                .unwrap_or("tool");
            let label = if tr.is_error { "error" } else { "result" };

            // Observe only consumes the normalized observation view.
            let observation = tr.observation();

            let mut body = format!("Tool {} {}:\n{}", name, label, observation);

            // Add storage reference hint if available
            if let Some(ref storage_ref) = tr.storage_ref {
                body.push_str(&format!(
                    "\n\nFull output saved to: {}",
                    storage_ref.path.display()
                ));
            }

            let tool_call_id = tr
                .call_id
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    warn!(
                        tool_name = %name,
                        "observe:missing_call_id generating synthetic id"
                    );
                    format!("call_{}", uuid6())
                });

            debug!(
                call_id = %tool_call_id,
                name = %name,
                is_error = tr.is_error,
                content_len = body.len(),
                "observe:convert → Message::Tool"
            );

            messages.push(Message::Tool {
                tool_call_id,
                content: ToolCallContent::text(body),
            });
        }
        let next_turn = state.turn_count.saturating_add(1);
        let new_state = ReActState {
            messages,
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: next_turn,
            ..state
        };
        let max_turns_reached = self.max_turns.map_or(false, |m| next_turn >= m);
        let (next, exit_reason) = if self.enable_loop && max_turns_reached {
            (Next::End, "max_turns_reached")
        } else if self.enable_loop && had_tool_calls {
            (Next::Continue, "loop_back_to_think")
        } else if self.enable_loop && !had_tool_calls {
            (Next::End, "no_tool_calls_final_answer")
        } else {
            (Next::Continue, "linear_next")
        };
        debug!(
            messages_before,
            messages_after = new_state.messages.len(),
            had_tool_calls,
            tool_results_consumed = state.tool_results.len(),
            turn = next_turn,
            exit_reason,
            "observe:exit"
        );
        for (i, msg) in new_state.messages.iter().enumerate().skip(messages_before.saturating_sub(2)) {
            debug!("  {}", message_summary(i, msg));
        }
        Ok((new_state, next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::state::{ReActState, ToolResult, ToolStorageRef};
    use loom_llm::ToolCall;
    use loom_llm::message::Message;
    use std::path::PathBuf;

    fn make_tool_result(call_id: &str, name: &str, content: &str, is_error: bool) -> ToolResult {
        ToolResult {
            call_id: Some(call_id.to_string()),
            name: Some(name.to_string()),
            content: content.to_string(),
            is_error,
            ..Default::default()
        }
    }

    fn make_empty_state() -> ReActState {
        ReActState::default()
    }

    #[test]
    fn test_observe_node_new_returns_correct_id() {
        let node = ObserveNode::new();
        assert_eq!(node.id(), "observe");
    }

    #[test]
    fn test_observe_node_default_equals_new() {
        let new_node = ObserveNode::new();
        let default_node = ObserveNode::default();
        assert_eq!(new_node.id(), default_node.id());
        assert_eq!(new_node.id(), "observe");
    }

    #[tokio::test]
    async fn test_observe_linear_no_tool_results_returns_continue() {
        let node = ObserveNode::new();
        let state = make_empty_state();
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        assert_eq!(new_state.messages.len(), 0);
    }

    #[tokio::test]
    async fn test_observe_linear_with_tool_results_merges_messages() {
        let node = ObserveNode::new();
        let mut state = make_empty_state();
        state.tool_results = vec![
            make_tool_result("call_1", "read_file", "file content", false),
            make_tool_result("call_2", "grep", "found matches", false),
        ];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        assert_eq!(new_state.messages.len(), 2);
        assert!(new_state.tool_results.is_empty());
        assert!(new_state.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn test_observe_loop_no_tool_calls_returns_end() {
        let node = ObserveNode::with_loop();
        let state = make_empty_state();
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::End));
        assert_eq!(new_state.turn_count, 1);
    }

    #[tokio::test]
    async fn test_observe_loop_with_tool_calls_returns_continue() {
        let node = ObserveNode::with_loop();
        let mut state = make_empty_state();
        state.tool_calls = vec![ToolCall::new("test_tool", "{}")];
        state.tool_results = vec![make_tool_result("call_1", "test_tool", "result", false)];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        assert_eq!(new_state.turn_count, 1);
    }

    #[tokio::test]
    async fn test_observe_loop_max_turns_reached_returns_end() {
        let node = ObserveNode::with_loop_max_turns(1);
        let state = make_empty_state();
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::End));
        assert_eq!(new_state.turn_count, 1); // turn_count was 0, incremented to 1
    }

    #[tokio::test]
    async fn test_observe_loop_max_turns_not_reached_returns_continue() {
        let node = ObserveNode::with_loop_max_turns(5);
        let mut state = make_empty_state();
        state.tool_calls = vec![ToolCall::new("test_tool", "{}")];
        state.tool_results = vec![make_tool_result("call_1", "test_tool", "result", false)];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        assert_eq!(new_state.turn_count, 1);
    }

    #[tokio::test]
    async fn test_observe_generates_synthetic_call_id_when_missing() {
        let node = ObserveNode::new();
        let mut state = make_empty_state();
        
        let result_without_call_id = ToolResult {
            call_id: None,
            name: Some("test_tool".to_string()),
            content: "content".to_string(),
            is_error: false,
            ..Default::default()
        };
        state.tool_results = vec![result_without_call_id];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        assert_eq!(new_state.messages.len(), 1);
        
        if let Message::Tool { tool_call_id, .. } = &new_state.messages[0] {
            assert!(tool_call_id.starts_with("call_"));
        } else {
            panic!("Expected Tool message");
        }
    }

    #[tokio::test]
    async fn test_observe_uses_name_for_tool_label() {
        let node = ObserveNode::new();
        let mut state = make_empty_state();
        state.tool_results = vec![make_tool_result("call_1", "read_file", "file content", false)];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        
        if let Message::Tool { content, .. } = &new_state.messages[0] {
            assert!(content.as_text().unwrap().contains("Tool read_file"));
        } else {
            panic!("Expected Tool message");
        }
    }

    #[tokio::test]
    async fn test_observe_error_label() {
        let node = ObserveNode::new();
        let mut state = make_empty_state();
        state.tool_results = vec![make_tool_result("call_1", "read_file", "file not found", true)];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        
        if let Message::Tool { content, .. } = &new_state.messages[0] {
            assert!(content.as_text().unwrap().contains("error"));
        } else {
            panic!("Expected Tool message");
        }
    }

    #[tokio::test]
    async fn test_observe_result_label() {
        let node = ObserveNode::new();
        let mut state = make_empty_state();
        state.tool_results = vec![make_tool_result("call_1", "read_file", "file content", false)];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        
        if let Message::Tool { content, .. } = &new_state.messages[0] {
            assert!(content.as_text().unwrap().contains("result"));
        } else {
            panic!("Expected Tool message");
        }
    }

    #[tokio::test]
    async fn test_observe_clears_tool_calls_and_results() {
        let node = ObserveNode::new();
        let mut state = make_empty_state();
        state.tool_calls = vec![ToolCall::new("test_tool", "{}")];
        state.tool_results = vec![make_tool_result("call_1", "test_tool", "result", false)];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(new_state.tool_calls.is_empty());
        assert!(new_state.tool_results.is_empty());
        assert!(matches!(next, Next::Continue));
    }

    #[tokio::test]
    async fn test_observe_increments_turn_count() {
        let node = ObserveNode::new();
        let mut state = make_empty_state();
        state.turn_count = 5;
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert_eq!(new_state.turn_count, 6);
        assert!(matches!(next, Next::Continue));
    }

    #[tokio::test]
    async fn test_observe_storage_ref_hint() {
        let node = ObserveNode::new();
        let mut state = make_empty_state();
        
        let mut tool_result = make_tool_result("call_1", "read_file", "file content", false);
        tool_result.storage_ref = Some(ToolStorageRef {
            path: PathBuf::from("/tmp/output.txt"),
            size: 1024,
            content_type: "text/plain".to_string(),
            encoding: "utf-8".to_string(),
            tool_name: "read_file".to_string(),
        });
        state.tool_results = vec![tool_result];
        
        let (new_state, next) = node.run(state).await.unwrap();
        assert!(matches!(next, Next::Continue));
        
        if let Message::Tool { content, .. } = &new_state.messages[0] {
            let content_str = content.as_text().unwrap();
            assert!(content_str.contains("Full output saved to:"));
            assert!(content_str.contains("/tmp/output.txt"));
        } else {
            panic!("Expected Tool message");
        }
    }
}

