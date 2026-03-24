//! Completion check node: use LLM to determine if the task is completed.
//!
//! This node examines the last N messages and uses an LLM to decide whether
//! the agent should continue working or end the conversation.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::llm::LlmClient;
use crate::message::Message;
use crate::state::ReActState;
use crate::Node;

/// Default system prompt for completion checking
const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a task completion evaluator.

Your job is to determine if the agent has completed the user's original task.

Analyze the last few messages in the conversation and decide:
- If the task is COMPLETE: respond with {"completed": true, "reason": "..."}
- If the task is INCOMPLETE: respond with {"completed": false, "reason": "..."}

Guidelines:
- If the last assistant message contains a final answer, solution, or clear conclusion → COMPLETE
- If the last assistant message asks follow-up questions or indicates more work needed → INCOMPLETE
- If there are pending tool calls or the task is clearly unfinished → INCOMPLETE
- If the user's original request has been fully addressed → COMPLETE

Respond ONLY with valid JSON, no other text."#;

/// Response from the completion check LLM call
#[derive(Debug, Serialize, Deserialize)]
struct CompletionResponse {
    completed: bool,
    reason: String,
}

/// Node that checks if the ReAct loop should continue or end.
///
/// This node is called when Think produces no tool calls. It uses an LLM
/// to examine recent messages and determine if the task is truly complete.
pub struct CompletionCheckNode {
    llm: Arc<dyn LlmClient>,
    max_iterations: usize,
    message_window: usize,
    system_prompt: String,
}

impl CompletionCheckNode {
    /// Creates a new completion check node with the given LLM client.
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self {
            llm,
            max_iterations: 10,
            message_window: 5,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
        }
    }

    /// Sets the maximum number of iterations before forcing an end.
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    /// Sets the number of recent messages to examine.
    pub fn with_message_window(mut self, window: usize) -> Self {
        self.message_window = window;
        self
    }

    /// Sets a custom system prompt for completion checking.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
    }

    /// Get the last N messages from the state
    fn get_recent_messages(&self, state: &ReActState) -> Vec<Message> {
        let start = state.messages.len().saturating_sub(self.message_window);
        state.messages[start..].to_vec()
    }

    /// Ask LLM to check if task is complete
    async fn check_completion(&self, _messages: Vec<Message>) -> Result<CompletionResponse, AgentError> {
        let check_messages = vec![
            Message::system(self.system_prompt.clone()),
            Message::user("Based on the recent conversation, is the task complete?"),
        ];

        let response = self.llm.invoke(&check_messages).await?;
        let content = response.content.trim();

        // Try to parse JSON response
        serde_json::from_str(content)
            .map_err(|e| {
                AgentError::ExecutionFailed(format!("Failed to parse completion check response: {}", e))
            })
    }
}

#[async_trait]
impl Node<ReActState> for CompletionCheckNode {
    fn id(&self) -> &str {
        "completion_check"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let ctx = RunContext::new(crate::memory::RunnableConfig::default());
        self.run_with_context(state, &ctx).await
    }

    async fn run_with_context(
        &self,
        mut state: ReActState,
        _ctx: &RunContext<ReActState>,
    ) -> Result<(ReActState, Next), AgentError> {
        // Safety check: don't exceed max iterations
        if state.turn_count as usize >= self.max_iterations {
            debug!(
                turn_count = state.turn_count,
                max_iterations = self.max_iterations,
                "Max iterations reached, ending"
            );
            state.should_continue = false;
            return Ok((state, Next::End));
        }

        // Get recent messages
        let recent_messages = self.get_recent_messages(&state);
        
        if recent_messages.is_empty() {
            debug!("No messages to analyze, ending");
            state.should_continue = false;
            return Ok((state, Next::End));
        }

        // Ask LLM if task is complete
        match self.check_completion(recent_messages).await {
            Ok(completion) => {
                debug!(
                    completed = completion.completed,
                    reason = %completion.reason,
                    turn_count = state.turn_count,
                    "Completion check result"
                );

                if completion.completed {
                    state.should_continue = false;
                    Ok((state, Next::End))
                } else {
                    // Increment turn count and continue
                    state.turn_count += 1;
                    state.should_continue = true;
                    Ok((state, Next::Continue))
                }
            }
            Err(e) => {
                warn!(error = %e, "LLM completion check failed, defaulting to continue");
                state.turn_count += 1;
                state.should_continue = true;
                Ok((state, Next::Continue))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::RunContext;
    use crate::llm::MockLlm;
    use crate::memory::RunnableConfig;

    #[tokio::test]
    async fn completion_check_ends_when_task_complete() {
        let llm = MockLlm::with_no_tool_calls(r#"{"completed": true, "reason": "Task finished"}"#);

        let node = CompletionCheckNode::new(Arc::new(llm))
            .with_max_iterations(10)
            .with_message_window(5);

        let state = ReActState {
            messages: vec![
                Message::user("What is 2+2?"),
                Message::assistant("The answer is 4."),
            ],
            turn_count: 1,
            ..Default::default()
        };

        let ctx = RunContext::<ReActState>::new(RunnableConfig::default());
        let (new_state, next) = node.run_with_context(state, &ctx).await.unwrap();

        assert!(!new_state.should_continue);
        assert!(matches!(next, Next::End));
    }

    #[tokio::test]
    async fn completion_check_continues_when_task_incomplete() {
        let llm = MockLlm::with_no_tool_calls(r#"{"completed": false, "reason": "More work needed"}"#);

        let node = CompletionCheckNode::new(Arc::new(llm))
            .with_max_iterations(10)
            .with_message_window(5);

        let state = ReActState {
            messages: vec![
                Message::user("List all files"),
                Message::assistant("I found some files..."),
            ],
            turn_count: 1,
            ..Default::default()
        };

        let ctx = RunContext::<ReActState>::new(RunnableConfig::default());
        let (new_state, next) = node.run_with_context(state, &ctx).await.unwrap();

        assert!(new_state.should_continue);
        assert!(matches!(next, Next::Continue));
        assert_eq!(new_state.turn_count, 2);
    }

    #[tokio::test]
    async fn completion_check_ends_at_max_iterations() {
        let llm = MockLlm::with_no_tool_calls(r#"{"completed": false, "reason": "Still working"}"#);

        let node = CompletionCheckNode::new(Arc::new(llm))
            .with_max_iterations(3)
            .with_message_window(5);

        let state = ReActState {
            messages: vec![Message::user("Do something")],
            turn_count: 3, // Already at max
            ..Default::default()
        };

        let ctx = RunContext::<ReActState>::new(RunnableConfig::default());
        let (new_state, next) = node.run_with_context(state, &ctx).await.unwrap();

        assert!(!new_state.should_continue);
        assert!(matches!(next, Next::End));
    }
}
