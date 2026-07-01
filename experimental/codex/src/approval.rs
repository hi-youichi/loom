use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::{mpsc, oneshot};
use crate::agent::OutputEvent;
use crate::stdio_loop::CodexNotification;
use serde_json::json;

/// Result of an approval request. Reserved for the Codex Daemon approval
/// protocol — `request()` is not yet wired into the event bridge but will be
/// once command-execution interception is implemented.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // reserved: Codex approval protocol (see docs/dev/loom-codex-daemon.md)
pub enum ApprovalResult {
    Approved,
    Denied,
}

pub struct ApprovalManager {
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    /// Output channel for sending requestApproval notifications to the client.
    /// Reserved — only used by `request()` which is not yet wired.
    #[allow(dead_code)] // reserved: Codex approval protocol
    output_tx: mpsc::Sender<OutputEvent>,
}

impl ApprovalManager {
    pub fn new(output_tx: mpsc::Sender<OutputEvent>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            output_tx,
        }
    }

    /// Sends requestApproval notification to client and waits for approve/deny response.
    ///
    /// Reserved — not yet called from the event bridge. Will be invoked when
    /// command-execution interception is implemented.
    #[allow(dead_code)] // reserved: Codex approval protocol
    pub async fn request(&self, call_id: String, command: String) -> ApprovalResult {
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            pending.insert(call_id.clone(), tx);
        }

        // Send thread/commandExecution/requestApproval notification to client
        let notif = CodexNotification {
            jsonrpc: "2.0",
            method: "thread/commandExecution/requestApproval".to_string(),
            params: json!({
                "commandExecutionId": call_id,
                "command": command,
            }),
        };
        let _ = self.output_tx.send(OutputEvent::Notification(notif)).await;

        // Wait for client response; treat channel errors as denial
        match rx.await {
            Ok(true) => ApprovalResult::Approved,
            _ => ApprovalResult::Denied,
        }
    }

    /// Called when client sends approve or deny for a pending command execution.
    /// If call_id is not found in the pending map, the call is silently ignored.
    pub fn resolve(&self, call_id: &str, approved: bool) {
        let mut pending = self.pending.lock().unwrap();
        if let Some(tx) = pending.remove(call_id) {
            let _ = tx.send(approved);
        }
        // If call_id is not found, do nothing — the request may have already timed out
        // or been resolved, so we silently ignore the stale resolve.
    }
}
