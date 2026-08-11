//! Built-in command state traits.

use loom_llm::message::Message;

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
