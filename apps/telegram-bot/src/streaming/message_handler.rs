use std::sync::Arc;

use tokio::sync::mpsc;

use crate::traits::{AgentRunContext, MessageSender};

#[derive(Debug, Clone)]
pub enum StreamCommand {
    Flush,
}

pub async fn stream_message_handler_simple(
    rx: mpsc::Receiver<StreamCommand>,
    _sender: Arc<dyn MessageSender>,
    _chat_id: i64,
    _settings: crate::config::StreamingConfig,
) -> String {
    stream_message_handler_with_context(
        rx,
        _sender,
        _chat_id,
        AgentRunContext::default(),
        _settings,
    )
    .await
}

pub async fn stream_message_handler_with_context(
    mut rx: mpsc::Receiver<StreamCommand>,
    _sender: Arc<dyn MessageSender>,
    chat_id: i64,
    _context: AgentRunContext,
    _settings: crate::config::StreamingConfig,
) -> String {
    // Simply drain commands until Flush or channel closes.
    // The actual reply is sent by agent_orchestrator after the agent run completes.
    #[allow(clippy::never_loop)]
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
