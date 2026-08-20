use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::{auth, ExtensionContext, ExtensionError, ExtensionHandler};
use crate::terminal::{TerminalManager, TerminalStatus};

const MAX_IDENTIFIER_LENGTH: usize = 256;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalRestartRequest {
    pub session_id: String,
    #[serde(default = "default_true")]
    pub preserve_history: bool,
    #[serde(default)]
    pub clear_screen: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalRestartResponse {
    pub session_id: String,
    pub restarted: bool,
    pub new_pid: u32,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalForceKillRequest {
    pub session_id: String,
    pub confirm_token: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalForceKillResponse {
    pub session_id: String,
    pub killed: bool,
    pub killed_pids: Vec<u32>,
    pub signal: String,
    pub message: String,
}

#[derive(Clone)]
pub struct TerminalExtHandler {
    terminal_mgr: Arc<TerminalManager>,
    replacements: Arc<RwLock<HashMap<String, String>>>,
}

impl TerminalExtHandler {
    pub fn new(terminal_mgr: Arc<TerminalManager>) -> Self {
        Self {
            terminal_mgr,
            replacements: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn actual_session_id(&self, session_id: &str) -> String {
        self.replacements
            .read()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| session_id.to_string())
    }

    async fn restart(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "terminal", "restart")?;
        let request: TerminalRestartRequest = object_params(params)?;
        validate_identifier("sessionId", &request.session_id)?;

        let actual_id = self.actual_session_id(&request.session_id).await;
        let session = self
            .terminal_mgr
            .get_terminal(&actual_id)
            .await
            .ok_or_else(|| ExtensionError::not_found("terminal session not found"))?;
        if matches!(&session.status, TerminalStatus::Released) {
            return Err(ExtensionError::not_found("terminal session was released"));
        }

        self.terminal_mgr
            .kill(&actual_id)
            .await
            .map_err(|e| internal_error(e.to_string()))?;

        let replacement = self
            .terminal_mgr
            .create_terminal(
                session.command,
                session.args,
                session.cwd,
                session.env,
                session.output_byte_limit,
            )
            .await
            .map_err(|e| internal_error(e.to_string()))?;

        if !request.preserve_history {
            self.terminal_mgr.append_output(&replacement, "").await;
        }
        if request.clear_screen {
            self.terminal_mgr
                .append_output(&replacement, "\u{1b}[2J\u{1b}[H")
                .await;
        }

        self.replacements
            .write()
            .await
            .insert(request.session_id.clone(), replacement);

        serde_json::to_value(TerminalRestartResponse {
            session_id: request.session_id,
            restarted: true,
            new_pid: 0,
            message: "Terminal restarted successfully.".into(),
        })
        .map_err(|e| internal_error(e.to_string()))
    }

    async fn force_kill(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "terminal", "force_kill")?;
        let request: TerminalForceKillRequest = object_params(params)?;
        validate_identifier("sessionId", &request.session_id)?;
        validate_identifier("confirmToken", &request.confirm_token)?;

        let actual_id = self.actual_session_id(&request.session_id).await;
        let session = self
            .terminal_mgr
            .get_terminal(&actual_id)
            .await
            .ok_or_else(|| ExtensionError::not_found("terminal session not found"))?;
        if matches!(&session.status, TerminalStatus::Released) {
            return Err(ExtensionError::not_found("terminal session was released"));
        }

        let already_terminated = !matches!(&session.status, TerminalStatus::Running);
        if !already_terminated {
            self.terminal_mgr
                .kill(&actual_id)
                .await
                .map_err(|e| internal_error(e.to_string()))?;
        }

        tracing::info!(
            action = "terminal.force_kill",
            session_id = %request.session_id,
            caller_connection_id = %ctx.connection_id,
            caller_identity = %ctx.principal,
            killed_pids = ?Vec::<u32>::new(),
            reason = "client_request",
            "terminal force-kill audit event"
        );

        serde_json::to_value(TerminalForceKillResponse {
            session_id: request.session_id,
            killed: true,
            killed_pids: Vec::new(),
            signal: "SIGKILL".into(),
            message: if already_terminated {
                "Process tree already terminated (no processes killed).".into()
            } else {
                "Process tree terminated.".into()
            },
        })
        .map_err(|e| internal_error(e.to_string()))
    }

    async fn create(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "terminal", "create")?;
        let command = param_str(&params, "command").unwrap_or_else(|| {
            if cfg!(windows) {
                "powershell".to_string()
            } else {
                "sh".to_string()
            }
        });
        let cwd = param_str(&params, "cwd").or_else(|| {
            ctx.working_directory
                .as_deref()
                .map(|p| p.to_string_lossy().into_owned())
        });
        let args: Vec<String> = params
            .get("args")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let env: Vec<(String, String)> = params
            .get("env")
            .and_then(|v| v.as_object())
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let limit = params.get("outputByteLimit").and_then(|v| v.as_u64());
        let terminal_id = self
            .terminal_mgr
            .create_terminal(
                command.clone(),
                args.clone(),
                cwd.map(std::path::PathBuf::from),
                env,
                limit,
            )
            .await
            .map_err(|e| internal_error(e.to_string()))?;
        Ok(json!({
            "terminalId": terminal_id,
            "command": command,
            "args": args,
        }))
    }

    async fn write(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "terminal", "write")?;
        let terminal_id = require_param_str(&params, "terminalId")?;
        let actual_id = self.actual_session_id(&terminal_id).await;
        let data = require_param_str(&params, "data")?;
        self.terminal_mgr
            .write_input(&actual_id, data.as_bytes())
            .await
            .map_err(|e| internal_error(e.to_string()))?;
        Ok(json!({ "ok": true }))
    }

    async fn resize(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "terminal", "resize")?;
        let terminal_id = require_param_str(&params, "terminalId")?;
        let actual_id = self.actual_session_id(&terminal_id).await;
        if self.terminal_mgr.get_status(&actual_id).await.is_none() {
            return Err(ExtensionError::not_found(format!(
                "terminal not found: {terminal_id}"
            )));
        }
        Ok(json!({ "ok": true }))
    }

    async fn close(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "terminal", "close")?;
        let terminal_id = require_param_str(&params, "terminalId")?;
        let actual_id = self.actual_session_id(&terminal_id).await;
        self.terminal_mgr
            .release(&actual_id)
            .await
            .map_err(|e| internal_error(e.to_string()))?;
        Ok(json!({ "ok": true }))
    }

    async fn output(&self, params: Value) -> Result<Value, ExtensionError> {
        let terminal_id = require_param_str(&params, "terminalId")?;
        let actual_id = self.actual_session_id(&terminal_id).await;
        let (full, truncated, status) = self
            .terminal_mgr
            .get_output(&actual_id)
            .await
            .ok_or_else(|| {
                ExtensionError::not_found(format!("terminal not found: {terminal_id}"))
            })?;
        let from = params
            .get("fromIndex")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let slice = if from < full.len() { &full[from..] } else { "" };
        Ok(json!({
            "output": slice,
            "fromIndex": from,
            "totalLength": full.len(),
            "truncated": truncated,
            "status": terminal_status_value(status),
        }))
    }

    async fn status(&self, params: Value) -> Result<Value, ExtensionError> {
        let terminal_id = require_param_str(&params, "terminalId")?;
        let actual_id = self.actual_session_id(&terminal_id).await;
        let status = self
            .terminal_mgr
            .get_status(&actual_id)
            .await
            .ok_or_else(|| {
                ExtensionError::not_found(format!("terminal not found: {terminal_id}"))
            })?;
        Ok(json!({
            "terminalId": terminal_id,
            "status": terminal_status_value(Some(status)),
        }))
    }
}

fn terminal_status_value(status: Option<TerminalStatus>) -> Value {
    match status {
        None => Value::Null,
        Some(TerminalStatus::Running) => json!("running"),
        Some(TerminalStatus::Completed { exit_code, signal }) => json!({
            "state": "completed",
            "exitCode": exit_code,
            "signal": signal,
        }),
        Some(TerminalStatus::Killed) => json!("killed"),
        Some(TerminalStatus::Released) => json!("released"),
    }
}

#[async_trait]
impl ExtensionHandler for TerminalExtHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "restart" => self.restart(params, ctx).await,
            "force_kill" => self.force_kill(params, ctx).await,
            "create" => self.create(params, ctx).await,
            "write" => self.write(params, ctx).await,
            "resize" => self.resize(params, ctx).await,
            "close" => self.close(params, ctx).await,
            "output" => self.output(params).await,
            "status" => self.status(params).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "restart": true,
            "force_kill": true,
            "create": true,
            "write": true,
            "resize": true,
            "close": true,
            "output": true,
            "status": true,
            "streaming": "global:terminal",
        })
    }
}

fn param_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn require_param_str(params: &Value, key: &str) -> Result<String, ExtensionError> {
    param_str(params, key).filter(|s| !s.trim().is_empty()).ok_or_else(|| {
        ExtensionError::invalid_params(format!("missing required parameter: {key}"))
    })
}

fn default_true() -> bool {
    true
}

fn object_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
    if !params.is_object() {
        return Err(ExtensionError::invalid_params("params must be an object"));
    }
    serde_json::from_value(params)
        .map_err(|e| ExtensionError::invalid_params(format!("invalid params: {e}")))
}

fn validate_identifier(field: &str, value: &str) -> Result<(), ExtensionError> {
    if value.trim().is_empty() {
        return Err(ExtensionError::invalid_params(format!(
            "{field} must not be empty"
        )));
    }
    if value.chars().count() > MAX_IDENTIFIER_LENGTH {
        return Err(ExtensionError::invalid_params(format!(
            "{field} is too long"
        )));
    }
    Ok(())
}

fn internal_error(message: impl Into<String>) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}
