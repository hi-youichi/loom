//! Build initial ReAct state from user message, optionally loading from checkpoint.

use crate::state::ReActState;
use checkpoint::{CheckpointError, Checkpointer, RunnableConfig};
use anureo_llm::message::{Message, UserContent};

use crate::runner_common::{load_from_checkpoint_or_build, resume_from_checkpoint};

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

/// Builds initial [`ReActState`] for workflow resume: loads from checkpoint
/// without appending a user message (the prompt is already in history).
pub async fn build_react_initial_state_for_resume(
    checkpointer: Option<&dyn Checkpointer<ReActState>>,
    runnable_config: Option<&RunnableConfig>,
    system_prompt: &str,
) -> Result<ReActState, CheckpointError> {
    resume_from_checkpoint(checkpointer, runnable_config, async move {
        Ok(ReActState {
            model_config: Default::default(),
            messages: vec![Message::system(system_prompt)],
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
    })
    .await
}
