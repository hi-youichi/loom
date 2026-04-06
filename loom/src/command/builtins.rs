//! Built-in command execution and state traits.
//!
//! Each state type implements the traits it supports, and the dispatcher calls the right one.
//! Sync commands (reset) run immediately; async commands (compact, summarize) require an LLM.

use crate::command::command::{Command, CommandResult};
use crate::compress::compaction::{build_summary_prompt, compact, prune};
use crate::compress::config::CompactionConfig;
use crate::error::AgentError;
use crate::llm::LlmClient;
use crate::message::{Message, UserContent};

pub trait ResetState {
    fn reset_context(&mut self);
}

pub trait CompactState {
    fn messages(&self) -> &[Message];
    fn set_messages(&mut self, messages: Vec<Message>);
    fn set_summary(&mut self, summary: String);
}

pub trait SummarizeState {
    fn messages(&self) -> &[Message];
    fn set_summary(&mut self, summary: String);
}

pub fn execute(cmd: Command, state: &mut dyn ResetState) -> CommandResult {
    match cmd {
        Command::ResetContext => {
            state.reset_context();
            CommandResult::Reply("Context cleared.".into())
        }
        Command::Compact { .. }
        | Command::Summarize
        | Command::Models { .. }
        | Command::ModelsUse { .. } => {
            CommandResult::PassThrough
        }
    }
}

pub async fn execute_async<S>(
    cmd: Command,
    state: &mut S,
    llm: &dyn LlmClient,
    compaction_config: &CompactionConfig,
) -> Result<CommandResult, AgentError>
where
    S: ResetState + CompactState + SummarizeState,
{
    match cmd {
        Command::ResetContext => {
            state.reset_context();
            Ok(CommandResult::Reply("Context cleared.".into()))
        }
        Command::Compact { instructions } => {
            let messages = CompactState::messages(state).to_vec();
            let pruned = prune(messages, compaction_config);
            let compacted = compact(&pruned, llm, compaction_config).await?;

            let summary = compacted.first().and_then(|m| match m {
                Message::System(s) => Some(s.clone()),
                _ => None,
            }).unwrap_or_default();

            let summary = if let Some(ref instr) = instructions {
                format!("{}\n\nFocus: {}", summary, instr)
            } else {
                summary
            };

            state.set_messages(compacted);
            CompactState::set_summary(state, summary);

            Ok(CommandResult::Reply("Context compacted.".into()))
        }
        Command::Summarize => {
            let messages = SummarizeState::messages(state);
            if messages.is_empty() {
                return Ok(CommandResult::Reply("Nothing to summarize.".into()));
            }

            let prompt = build_summary_prompt(messages);
            let summary_msgs = vec![Message::User(UserContent::Text(prompt))];
            let response = llm.invoke(&summary_msgs).await?;
            let content = response.content;

            SummarizeState::set_summary(state, content.clone());

            Ok(CommandResult::Reply(content))
        }
        Command::Models { .. } | Command::ModelsUse { .. } => {
            Ok(CommandResult::PassThrough)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_sync_reset_returns_reply() {
        struct TestState {
            messages: Vec<Message>,
            tool_calls: Vec<String>,
            turn_count: u32,
        }
        impl ResetState for TestState {
            fn reset_context(&mut self) {
                self.messages.clear();
                self.tool_calls.clear();
                self.turn_count = 0;
            }
        }
        let mut s = TestState {
            messages: vec![Message::user("hi"), Message::System("sys".into())],
            tool_calls: vec!["call1".into()],
            turn_count: 5,
        };
        let result = execute(Command::ResetContext, &mut s);
        assert_eq!(result, CommandResult::Reply("Context cleared.".into()));
        assert!(s.messages.is_empty());
        assert!(s.tool_calls.is_empty());
        assert_eq!(s.turn_count, 0);
    }

    #[test]
    fn execute_sync_passes_through_compact_summarize_models() {
        struct DummyState;
        impl ResetState for DummyState {
            fn reset_context(&mut self) {}
        }
        assert_eq!(
            execute(Command::Compact { instructions: None }, &mut DummyState),
            CommandResult::PassThrough
        );
        assert_eq!(execute(Command::Summarize, &mut DummyState), CommandResult::PassThrough);
        assert_eq!(execute(Command::Models { query: None }, &mut DummyState), CommandResult::PassThrough);
        assert_eq!(execute(Command::ModelsUse { model_id: "gpt".into() }, &mut DummyState), CommandResult::PassThrough);
    }
}
