use std::sync::Arc;

use tokio::sync::mpsc;

use crate::traits::AgentRunContext;

#[derive(Debug, Clone)]
pub enum StreamCommand {
    Flush,
}

pub struct MessageState {
    pub msg_id: Option<i32>,
    _ack_message_id: Option<i32>,
    _user_message_id: Option<i32>,
}

impl MessageState {
    pub fn new(_settings: &crate::config::StreamingConfig, context: AgentRunContext) -> Self {
        Self {
            msg_id: None,
            _ack_message_id: context.ack_message_id,
            _user_message_id: context.user_message_id,
        }
    }
}

pub async fn stream_message_handler_simple(
    rx: mpsc::Receiver<StreamCommand>,
    _sender: Arc<dyn crate::traits::MessageSender>,
    _chat_id: i64,
    _settings: crate::config::StreamingConfig,
) -> String {
    stream_message_handler_with_context(rx, _sender, _chat_id, AgentRunContext::default(), _settings)
        .await
}

pub async fn stream_message_handler(
    rx: mpsc::Receiver<StreamCommand>,
    _sender: Arc<dyn crate::traits::MessageSender>,
    _chat_id: i64,
    _context: AgentRunContext,
    settings: crate::config::Settings,
) -> String {
    stream_message_handler_with_context(rx, _sender, _chat_id, _context, settings.streaming).await
}

pub async fn stream_message_handler_with_context(
    mut rx: mpsc::Receiver<StreamCommand>,
    _sender: Arc<dyn crate::traits::MessageSender>,
    chat_id: i64,
    _context: AgentRunContext,
    _settings: crate::config::StreamingConfig,
) -> String {
    // Simply drain commands until Flush or channel closes.
    // The actual reply is sent by agent_orchestrator after the agent run completes.
    while let Some(cmd) = rx.recv().await {
        match cmd {
            StreamCommand::Flush => break,
        }
    }

    tracing::debug!(
        chat_id,
        "Stream message handler completed (no intermediate display)"
    );

    String::new()
}
