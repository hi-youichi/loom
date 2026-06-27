//! `ReverseRpcResponder` + `PermissionPolicy` — harness side of Agent→Client
//! JSON-RPC calls (plan 026 §2.1.4).
//!
//! Reverse RPCs the harness must answer:
//! - `session/request_permission` — IDE permission prompt
//! - `fs/read_text_file` / `fs/write_text_file` — IDE fs backend
//! - `terminal/create` / `terminal/output` / `terminal/kill` — IDE terminal
//!
//! Phase 1 ships the type + permission policy; fs/terminal tables land in
//! Phase 2 (Mega) and Phase 3 (M4 micro) respectively.

// Phase 1 only exercises `PermissionPolicy::AllowOnce`; allow dead code
// for variants + fields Phase 2 / Phase 3 will use.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionPolicy {
    /// Approve every request_permission call.
    AllowOnce,
    /// Reject every request_permission call.
    DenyOnce,
    /// Mark the call as cancelled (matches Loom `StopReason::Cancelled`).
    Cancelled,
    /// Do not respond (used to verify the agent does not hang on missing replies).
    Timeout,
}

#[derive(Debug)]
pub struct ReverseRpcResponder {
    permission_policy: Mutex<PermissionPolicy>,
    fs_files: Mutex<HashMap<PathBuf, String>>,
    terminal_outputs: Mutex<HashMap<String, String>>,
    permission_decisions: Mutex<Vec<(String, String)>>,
}

impl ReverseRpcResponder {
    pub fn new() -> Self {
        Self {
            permission_policy: Mutex::new(PermissionPolicy::AllowOnce),
            fs_files: Mutex::new(HashMap::new()),
            terminal_outputs: Mutex::new(HashMap::new()),
            permission_decisions: Mutex::new(Vec::new()),
        }
    }

    pub fn set_permission_policy(&self, p: PermissionPolicy) {
        *self.permission_policy.lock().unwrap() = p;
    }

    pub fn current_permission_policy(&self) -> PermissionPolicy {
        *self.permission_policy.lock().unwrap()
    }

    pub fn seed_file(&self, path: PathBuf, content: String) {
        self.fs_files.lock().unwrap().insert(path, content);
    }

    pub fn read_file(&self, path: &std::path::Path) -> Option<String> {
        self.fs_files.lock().unwrap().get(path).cloned()
    }

    pub fn seed_terminal_output(&self, terminal_id: String, output: String) {
        self.terminal_outputs
            .lock()
            .unwrap()
            .insert(terminal_id, output);
    }

    pub fn terminal_output(&self, terminal_id: &str) -> Option<String> {
        self.terminal_outputs.lock().unwrap().get(terminal_id).cloned()
    }

    /// Build the JSON-RPC `result` value to send back to the agent for a
    /// permission request. Records the decision for later assertions.
    pub fn answer_permission(
        &self,
        tool_call_id: &str,
        tool_name: &str,
    ) -> serde_json::Value {
        let policy = self.current_permission_policy();
        self.permission_decisions
            .lock()
            .unwrap()
            .push((tool_call_id.to_string(), tool_name.to_string()));
        match policy {
            PermissionPolicy::AllowOnce => serde_json::json!({
                "outcome": {
                    "outcome": "selected",
                    "optionId": "allow_always"
                }
            }),
            PermissionPolicy::DenyOnce => serde_json::json!({
                "outcome": {
                    "outcome": "selected",
                    "optionId": "reject_once"
                }
            }),
            PermissionPolicy::Cancelled => serde_json::json!({
                "outcome": {"outcome": "cancelled"}
            }),
            PermissionPolicy::Timeout => serde_json::json!({
                "outcome": {"outcome": "cancelled"}
            }),
        }
    }

    pub fn permission_decisions(&self) -> Vec<(String, String)> {
        self.permission_decisions.lock().unwrap().clone()
    }
}

impl Default for ReverseRpcResponder {
    fn default() -> Self {
        Self::new()
    }
}