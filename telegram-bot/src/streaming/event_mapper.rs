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
        StreamCommand::StartThink { .. } => "StartThink",
        StreamCommand::StartAct { .. } => "StartAct",
        StreamCommand::ThinkContent { .. } => "ThinkContent",
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
    match tx.try_send(command) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::error!(
                "Stream command send failed (handler receiver closed?): {}",
                command_kind
            );
        }
        Err(mpsc::error::TrySendError::Full(command)) => match priority {
            CommandPriority::BestEffort => {
                tracing::debug!(
                    "Dropped best-effort stream command due to full queue: {}",
                    command_kind
                );
            }
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
            }
        },
    }
}

pub(crate) struct PendingToolArguments {
    by_call_id: HashMap<String, String>,
    by_name: HashMap<String, VecDeque<String>>,
}

impl PendingToolArguments {
    pub(crate) fn new() -> Self {
        Self {
            by_call_id: HashMap::new(),
            by_name: HashMap::new(),
        }
    }

    fn remember(&mut self, call_id: &Option<String>, tool_name: &str, arguments: &serde_json::Value) {
        let arguments_text = arguments.to_string();
        if let Some(id) = call_id {
            self.by_call_id.insert(id.clone(), arguments_text);
            return;
        }
        self.by_name
            .entry(tool_name.to_string())
            .or_default()
            .push_back(arguments_text);
    }

    fn take_for_start(&mut self, call_id: &Option<String>, tool_name: &str) -> Option<String> {
        if let Some(id) = call_id {
            if let Some(arguments) = self.by_call_id.remove(id) {
                return Some(arguments);
            }
        }
        self.by_name
            .get_mut(tool_name)
            .and_then(|queue| queue.pop_front())
    }
}

type PhaseStateInner = (String, u32, u32);

/// Bridges Loom stream events into the Telegram streaming channel.
pub(crate) struct StreamEventMapper {
    tx: mpsc::Sender<StreamCommand>,
    phase_state: Arc<std::sync::RwLock<PhaseStateInner>>,
    pending_tool_args: Arc<std::sync::RwLock<PendingToolArguments>>,
    show_think: bool,
    show_act: bool,
}

impl StreamEventMapper {
    pub(crate) fn new(
        tx: mpsc::Sender<StreamCommand>,
        show_think: bool,
        show_act: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            tx,
            phase_state: Arc::new(std::sync::RwLock::new((String::new(), 0, 0))),
            pending_tool_args: Arc::new(std::sync::RwLock::new(PendingToolArguments::new())),
            show_think,
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
                    if node_id == "think" && self.show_think =>
                {
                    let think_count = {
                        let mut ps = self.phase_state.write().unwrap();
                        ps.1 += 1;
                        ps.0 = "think".to_string();
                        ps.1
                    };
                    send_stream_command(
                        &self.tx,
                        StreamCommand::StartThink { count: think_count },
                        CommandPriority::Critical,
                    );
                }

                AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. })
                    if node_id == "act" && self.show_act =>
                {
                    let act_count = {
                        let mut ps = self.phase_state.write().unwrap();
                        ps.2 += 1;
                        ps.0 = "act".to_string();
                        ps.2
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
                        "think" if self.show_think => {
                            send_stream_command(
                                &self.tx,
                                StreamCommand::ThinkContent {
                                    content: chunk.content.clone(),
                                },
                                CommandPriority::BestEffort,
                            );
                        }
                        "act" if self.show_act => {
                            send_stream_command(
                                &self.tx,
                                StreamCommand::ActContent {
                                    content: chunk.content.clone(),
                                },
                                CommandPriority::BestEffort,
                            );
                        }
                        _ => {
                            let phase = self.phase_state.read().unwrap().0.clone();
                            match phase.as_str() {
                                "think" if self.show_think => {
                                    send_stream_command(
                                        &self.tx,
                                        StreamCommand::ThinkContent {
                                            content: chunk.content.clone(),
                                        },
                                        CommandPriority::BestEffort,
                                    );
                                }
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
                    }
                }

                AnyStreamEvent::React(loom::StreamEvent::ToolStart { call_id, name }) => {
                    if self.show_act {
                        let tool_arguments = self
                            .pending_tool_args
                            .write()
                            .unwrap()
                            .take_for_start(call_id, name);
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
                    call_id,
                    name,
                    arguments,
                }) => {
                    if self.show_act {
                        self.pending_tool_args
                            .write()
                            .unwrap()
                            .remember(call_id, name, arguments);
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
