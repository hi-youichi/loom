//! ACP Agent implementation: maps protocol requests to Loom execution.
//!
//! [`LoomAcpAgent`] implements `agent_client_protocol::Agent` and maps ACP requests
//! to Loom sessions and execution. See [`crate::protocol`] for protocol and behavior details.

use crate::content::content_blocks_to_message;
use crate::session::{SessionId as OurSessionId, SessionStore};
use crate::stream_bridge::{loom_event_to_updates, stream_update_to_session_notification};
use agent_client_protocol::{
    Agent, AuthenticateRequest, AuthenticateResponse, CancelNotification, Implementation,
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SessionId, SessionNotification, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, StopReason,
};
use async_trait::async_trait;
use loom::{run_agent_with_options, AnyStreamEvent, RunCmd, RunError, RunOptions};
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Handle for Loom as an ACP Agent. Implements [`Agent`], holds the session store.
/// If [`session_update_tx`](Self::session_update_tx) is set, prompt execution sends
/// session/update notifications through this channel.
#[derive(Debug)]
pub struct LoomAcpAgent {
    pub(crate) sessions: SessionStore,
    /// If Some, on_event during prompt converts stream events to SessionNotification and try_sends here.
    pub(crate) session_update_tx: Option<mpsc::Sender<SessionNotification>>,
}

impl LoomAcpAgent {
    /// Construct a new Agent instance (no session/update sending).
    pub fn new() -> Self {
        Self {
            sessions: SessionStore::new(),
            session_update_tx: None,
        }
    }

    /// Construct an Agent with a session/update sender for the stdio loop to push stream updates to the client.
    pub fn with_session_update_tx(tx: mpsc::Sender<SessionNotification>) -> Self {
        Self {
            sessions: SessionStore::new(),
            session_update_tx: Some(tx),
        }
    }

    /// Returns read-only access to the session store.
    #[inline]
    pub fn sessions(&self) -> &SessionStore {
        &self.sessions
    }
}

impl Default for LoomAcpAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl Agent for LoomAcpAgent {
    async fn initialize(&self, args: InitializeRequest) -> agent_client_protocol::Result<InitializeResponse> {
        Ok(InitializeResponse::new(args.protocol_version)
            .agent_info(Implementation::new("loom", env!("CARGO_PKG_VERSION"))))
    }

    async fn authenticate(
        &self,
        _args: AuthenticateRequest,
    ) -> agent_client_protocol::Result<AuthenticateResponse> {
        Ok(AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        args: NewSessionRequest,
    ) -> agent_client_protocol::Result<NewSessionResponse> {
        let working_directory = Some(args.cwd.clone());
        let our_id = self.sessions.create(working_directory);
        let session_id = SessionId::new(our_id.as_str().to_string());
        let current_model = std::env::var("MODEL")
            .unwrap_or_else(|_| std::env::var("OPENAI_MODEL").unwrap_or_default());
        let config_options = build_model_config_options(&current_model)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
        Ok(NewSessionResponse::new(session_id).config_options(Some(config_options)))
    }

    async fn cancel(
        &self,
        args: CancelNotification,
    ) -> agent_client_protocol::Result<()> {
        let key = OurSessionId::new(args.session_id.to_string());
        self.sessions.set_cancelled(key);
        Ok(())
    }

    async fn set_session_config_option(
        &self,
        args: SetSessionConfigOptionRequest,
    ) -> agent_client_protocol::Result<SetSessionConfigOptionResponse> {
        let key = OurSessionId::new(args.session_id.to_string());
        if self.sessions.get(&key).is_none() {
            return Err(agent_client_protocol::Error::new(-32602, "unknown session"));
        }
        let config_id_str = args.config_id.to_string();
        let current_model = if config_id_str == "model" {
            let value_str = args.value.to_string();
            self.sessions
                .update_session_config(&key, |c| c.model = Some(value_str.clone()));
            value_str
        } else {
            return Err(agent_client_protocol::Error::new(
                -32602,
                format!("unsupported config_id: {}", config_id_str),
            ));
        };
        build_set_session_config_option_response(&current_model)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))
    }

    async fn prompt(&self, args: PromptRequest) -> agent_client_protocol::Result<PromptResponse> {
        let key = OurSessionId::new(args.session_id.to_string());
        let entry = self
            .sessions
            .get(&key)
            .ok_or_else(|| agent_client_protocol::Error::new(-32602, "unknown session"))?;

        let message = content_blocks_to_message(args.prompt.as_slice())
            .map_err(|_| agent_client_protocol::Error::new(-32602, "content_blocks parse failed"))?;

        let working_folder = entry
            .working_directory
            .clone()
            .unwrap_or_else(|| PathBuf::from(loom::DEFAULT_WORKING_FOLDER));

        let opts = RunOptions {
            message,
            working_folder: Some(working_folder),
            session_id: None,
            thread_id: Some(entry.thread_id.clone()),
            role_file: None,
            agent: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 4096,
            output_json: false,
            model: entry.session_config.model.clone(),
            mcp_config_path: None,
        };

        let session_id = args.session_id.clone();
        let tx = self.session_update_tx.clone();
        let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> = tx.map(|sender| {
            let closure = move |ev: AnyStreamEvent| {
                let updates = loom_event_to_updates(&ev);
                for u in &updates {
                    if let Some(notif) = stream_update_to_session_notification(&session_id, u) {
                        let _ = sender.try_send(notif);
                    }
                }
            };
            Box::new(closure) as Box<dyn FnMut(AnyStreamEvent) + Send>
        });

        match run_agent_with_options(&opts, &RunCmd::React, on_event).await {
            Ok(_reply) => Ok(PromptResponse::new(StopReason::EndTurn)),
            Err(e) => {
                tracing::error!(session_id = %args.session_id, error = %e, "run_agent failed");
                Err(map_run_error(e))
            }
        }
    }
}

fn map_run_error(e: RunError) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(e.to_string())
}

/// Build config_options array with a single "model" option (protocol types are non_exhaustive, so we construct via serde).
/// SessionConfigOption has kind flattened; SessionConfigKind uses tag "type" → "type": "select" and SessionConfigSelect fields at top level (camelCase).
fn build_model_config_options(
    current_model: &str,
) -> Result<Vec<agent_client_protocol::SessionConfigOption>, serde_json::Error> {
    let json = serde_json::json!([
        {
            "id": "model",
            "name": "Model",
            "description": "LLM model for this session.",
            "category": "model",
            "type": "select",
            "currentValue": current_model,
            "options": []
        }
    ]);
    serde_json::from_value(json)
}

/// Build SetSessionConfigOptionResponse with a single "model" option (protocol types are non_exhaustive, so we construct via serde).
fn build_set_session_config_option_response(
    current_model: &str,
) -> Result<SetSessionConfigOptionResponse, serde_json::Error> {
    let config_options = build_model_config_options(current_model)?;
    let json = serde_json::json!({
        "config_options": config_options,
        "meta": None::<()>
    });
    serde_json::from_value(json)
}
