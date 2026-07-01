use std::sync::Arc;

use loom_llm::Message;
use loom_stream::ToolStreamWriter;
use crate::active_operation::RunCancellation;
use loom_stream::TypedAnyStreamEvent;

#[derive(Clone, Default)]
pub struct ToolCallContext {
    pub recent_messages: Vec<Message>,
    pub stream_writer: Option<ToolStreamWriter>,
    pub any_stream_event_sender: Option<Arc<dyn Fn(TypedAnyStreamEvent) + Send + Sync>>,
    pub thread_id: Option<String>,
    pub user_id: Option<String>,
    pub acp_session_id: Option<String>,
    pub depth: u32,
    pub run_cancellation: Option<RunCancellation>,
}

impl ToolCallContext {
    pub fn new(recent_messages: Vec<Message>) -> Self {
        Self {
            recent_messages,
            ..Default::default()
        }
    }
}
