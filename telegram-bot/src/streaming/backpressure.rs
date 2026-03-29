use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use crate::streaming::message_handler::StreamCommand;

#[derive(Clone, Copy, Debug)]
pub(crate) enum CommandPriority {
    Critical,
    BestEffort,
}

pub(crate) struct ChannelBackpressure {
    tx: mpsc::Sender<StreamCommand>,
    dropped_best_effort: Arc<AtomicU64>,
}

impl ChannelBackpressure {
    pub fn new(tx: mpsc::Sender<StreamCommand>) -> Self {
        Self {
            tx,
            dropped_best_effort: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn send(&self, cmd: StreamCommand, priority: CommandPriority) {
        match priority {
            CommandPriority::Critical => {
                match self.tx.blocking_send(cmd) {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::warn!("Critical command dropped: {:?}", e.0);
                    }
                }
            }
            CommandPriority::BestEffort => {
                if self.tx.try_send(cmd).is_err() {
                    self.dropped_best_effort.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
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
    let bp = ChannelBackpressure::new(tx.clone());
    bp.send(command, priority);
}
