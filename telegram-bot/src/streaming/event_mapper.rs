//! Maps Loom [`AnyStreamEvent`] into Telegram UI [`StreamCommand`]s (Adapter pattern).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use loom::AnyStreamEvent;
use tokio::sync::mpsc;

use crate::streaming::message_handler::StreamCommand;

#[derive(Clone, Copy, Debug)]
pub(crate) enum CommandPriority {
    Critical,
    BestEffort,
}

fn stream_command_kind(command: &StreamCommand) -> &'static str {
    match command {
        StreamCommand::StartAct { .. } => "StartAct",
        StreamCommand::ActContent { .. } => "ActContent",
        StreamCommand::ToolStart { .. } => "ToolStart",
        StreamCommand::ToolEnd { .. } => "ToolEnd",
        StreamCommand::Flush => "Flush",
    }
}

pub(crate) fn send_stream_command(
    tx: &mpsc::Sender<StreamCommand>,
    command: StreamCommand,
    priority: CommandPriority,
) {
    let command_kind = stream_command_kind(&command);
    match priority {
        CommandPriority::Critical => {
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                if let Err(send_error) = tx_clone.send(command).await {
                    tracing::error!(
                        "Stream command send failed after queue drain (handler receiver closed?): {}",
                        send_error
                    );
                }
            });
            tracing::debug!(command = command_kind, "Sent critical command");
        }
        CommandPriority::BestEffort => {
            if tx.try_send(command).is_ok() {
                tracing::debug!(command = command_kind, "Sent best-effort command");
            } else {
                tracing::debug!(command = command_kind, "Dropped best-effort command (channel full)");
            }
        }
    }
}

struct PendingToolArguments {
    by_call_id: HashMap<String, String>,
    by_name: HashMap<String, VecDeque<String>>,
}

impl PendingToolArguments {
    fn new() -> Self {
        Self {
            by_call_id: HashMap::new(),
            by_name: HashMap::new(),
        }
    }

    fn remember(&mut self, call_id: Option<String>, tool_name: &str, arguments: serde_json::Value) {
        let arguments_text = arguments.to_string();
        if let Some(id) = call_id {
            self.by_call_id.insert(id, arguments_text);
            return;
        }
        self.by_name
            .entry(tool_name.to_string())
            .or_default()
            .push_back(arguments_text);
    }

    fn take_for_start(&mut self, call_id: Option<String>, tool_name: &str) -> Option<String> {
        if let Some(id) = call_id {
            if let Some(arguments) = self.by_call_id.remove(&id) {
                return Some(arguments);
            }
        }
        self.by_name
            .get_mut(tool_name)
            .and_then(|queue| queue.pop_front())
    }
}

type PhaseStateInner = (String, u32);

/// Bridges Loom stream events into the Telegram streaming channel.
pub(crate) struct StreamEventMapper {
    tx: mpsc::Sender<StreamCommand>,
    phase_state: Arc<std::sync::RwLock<PhaseStateInner>>,
    pending_tool_args: Arc<std::sync::RwLock<PendingToolArguments>>,
    show_act: bool,
}

impl StreamEventMapper {
    pub(crate) fn new(
        tx: mpsc::Sender<StreamCommand>,
        show_act: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            tx,
            phase_state: Arc::new(std::sync::RwLock::new((String::new(), 0))),
            pending_tool_args: Arc::new(std::sync::RwLock::new(PendingToolArguments::new())),
            show_act,
        })
    }

    pub(crate) fn boxed_callback(self: &Arc<Self>) -> Box<dyn FnMut(AnyStreamEvent) + Send> {
        let inner = Arc::clone(self);
        Box::new(move |ev| inner.map_event(ev))
    }

    fn map_event(&self, ev: AnyStreamEvent) {
        tokio::task::block_in_place(|| {
            match &ev {
                AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. })
                    if node_id == "act" && self.show_act =>
                {
                    let act_count = {
                        let mut ps = self.phase_state.write().expect("phase_state lock poisoned");
                        ps.1 += 1;
                        ps.0 = "act".to_string();
                        ps.1
                    };
                    send_stream_command(
                        &self.tx,
                        StreamCommand::StartAct { count: act_count },
                        CommandPriority::Critical,
                    );
                }

                AnyStreamEvent::React(loom::StreamEvent::Messages {
                    chunk,
                    metadata,
                }) => {
                    if chunk.content.is_empty() {
                        return;
                    }
                    match metadata.loom_node.as_str() {
                        "act" if self.show_act => {
                            send_stream_command(
                                &self.tx,
                                StreamCommand::ActContent {
                                    content: chunk.content.clone(),
                                },
                                CommandPriority::BestEffort,
                            );
                        }
                        _ => {}
                    }
                }

                AnyStreamEvent::React(loom::StreamEvent::ToolStart {
                    call_id: tool_call_id,
                    name,
                }) => {
                    if self.show_act {
                        let tool_arguments = self
                            .pending_tool_args
                            .write()
                            .expect("pending_tool_args lock poisoned")
                            .take_for_start(tool_call_id.clone(), name);
                        send_stream_command(
                            &self.tx,
                            StreamCommand::ToolStart {
                                name: name.clone(),
                                arguments: tool_arguments,
                            },
                            CommandPriority::Critical,
                        );
                    }
                }

                AnyStreamEvent::React(loom::StreamEvent::ToolCall {
                    call_id: tool_call_id,
                    name,
                    arguments,
                }) => {
                    if self.show_act {
                        self.pending_tool_args
                            .write()
                            .expect("pending_tool_args lock poisoned")
                            .remember(tool_call_id.clone(), name, arguments.clone());
                    }
                }

                AnyStreamEvent::React(loom::StreamEvent::ToolEnd {
                    name,
                    result,
                    is_error,
                    ..
                }) => {
                    if self.show_act {
                        send_stream_command(
                            &self.tx,
                            StreamCommand::ToolEnd {
                                name: name.clone(),
                                result: result.clone(),
                                is_error: *is_error,
                            },
                            CommandPriority::Critical,
                        );
                    }
                }

                _ => {}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn best_effort_command_is_dropped_when_channel_is_full() {
        let (tx, mut rx) = mpsc::channel::<StreamCommand>(1);
        tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();

        send_stream_command(
            &tx,
            StreamCommand::ActContent {
                content: "chunk".to_string(),
            },
            CommandPriority::BestEffort,
        );

        let first = rx.recv().await.unwrap();
        assert!(matches!(first, StreamCommand::StartAct { count: 1 }));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), rx.recv())
                .await
                .is_err(),
            "best-effort command should be dropped when queue is full"
        );
    }

    #[tokio::test]
    async fn critical_command_waits_for_queue_drain_when_channel_is_full() {
        let (tx, mut rx) = mpsc::channel::<StreamCommand>(1);
        tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();

        send_stream_command(
            &tx,
            StreamCommand::ToolEnd {
                name: "ls".to_string(),
                result: "ok".to_string(),
                is_error: false,
            },
            CommandPriority::Critical,
        );

        let first = rx.recv().await.unwrap();
        assert!(matches!(first, StreamCommand::StartAct { count: 1 }));

        let second = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .expect("critical command should be sent after queue drains")
            .expect("channel should still be open");
        assert!(matches!(second, StreamCommand::ToolEnd { .. }));
    }
}
