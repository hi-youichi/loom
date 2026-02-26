//! Build initial ReAct state from user message, optionally loading from checkpoint.

use crate::memory::{CheckpointError, Checkpointer, RunnableConfig};
use crate::message::Message;
use crate::runner_common::load_from_checkpoint_or_build;
use crate::state::ReActState;

use crate::agent::react::REACT_SYSTEM_PROMPT;

/// Builds initial [`ReActState`] for a user message, loading from checkpoint when available.
pub async fn build_react_initial_state(
    user_message: &str,
    checkpointer: Option<&dyn Checkpointer<ReActState>>,
    runnable_config: Option<&RunnableConfig>,
    system_prompt: Option<&str>,
) -> Result<ReActState, CheckpointError> {
    let prompt = system_prompt.unwrap_or(REACT_SYSTEM_PROMPT);
    let user_message_owned = user_message.to_string();
    load_from_checkpoint_or_build(
        checkpointer,
        runnable_config,
        user_message,
        async move {
            Ok(ReActState {
                messages: vec![Message::system(prompt), Message::user(user_message_owned)],
                tool_calls: vec![],
                tool_results: vec![],
                turn_count: 0,
                approval_result: None,
                usage: None,
                total_usage: None,
                message_count_after_last_think: None,
            })
        },
        |mut state, msg| {
            state.messages.push(Message::user(msg));
            state.tool_calls = vec![];
            state.tool_results = vec![];
            state
        },
    )
    .await
}
