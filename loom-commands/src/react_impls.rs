//! ReActState implementations of command traits.

use loom_llm::message::Message;
use loom_stream::state::ReActState;

use crate::command_traits::{CompactState, ResetState, SummarizeState};

impl ResetState for ReActState {
    fn reset_context(&mut self) {
        let system = self.messages.iter().find(|m| matches!(m, Message::System(_))).cloned();
        self.messages.clear();
        if let Some(sys) = system {
            self.messages.push(sys);
        }
        self.tool_calls.clear();
        self.tool_results.clear();
        self.last_reasoning_content = None;
        self.turn_count = 0;
        self.summary = None;
        self.think_count = 0;
        self.message_count_after_last_think = None;

        self.should_continue = true;
    }
}

impl CompactState for ReActState {
    fn messages(&self) -> &[Message] {
        &self.messages
    }
    fn set_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }
    fn set_summary(&mut self, summary: String) {
        self.summary = Some(summary);
    }
}

impl SummarizeState for ReActState {
    fn messages(&self) -> &[Message] {
        &self.messages
    }
    fn set_summary(&mut self, summary: String) {
        self.summary = Some(summary);
    }
}
