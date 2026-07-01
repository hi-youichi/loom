//! Build initial ReAct state from user message, optionally loading from checkpoint.

use checkpoint::{CheckpointError, Checkpointer, RunnableConfig};
use loom_llm::message::{Message, UserContent};
use loom_stream::state::ReActState;

use crate::runner_common::load_from_checkpoint_or_build;

/// Builds initial [`ReActState`] for a user message, loading from checkpoint when available.
pub async fn build_react_initial_state(
    user_message: &UserContent,
    checkpointer: Option<&dyn Checkpointer<ReActState>>,
    runnable_config: Option<&RunnableConfig>,
    system_prompt: &str,
) -> Result<ReActState, CheckpointError> {
    let user_message_owned = user_message.clone();
    load_from_checkpoint_or_build(
        checkpointer,
        runnable_config,
        user_message,
        async move {
Ok(ReActState {
            model_config: Default::default(),
            messages: vec![
                Message::system(system_prompt),
                Message::user(user_message_owned),
            ],
            last_reasoning_content: None,
            tool_calls: vec![],
            tool_results: vec![],
            turn_count: 0,
            usage: None,
            total_usage: None,
            message_count_after_last_think: None,
            summary: None,
            think_count: 0,
            should_continue: true,
            force_compact: false,
        })
        },
        |mut state, msg: UserContent| {
            state.messages.push(Message::user(msg));
            state.tool_calls = vec![];
            state.tool_results = vec![];
            state
        },
    )
    .await
}