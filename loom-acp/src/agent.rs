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
    PromptResponse, SessionId, SessionNotification, StopReason,
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
        Ok(NewSessionResponse::new(SessionId::new(our_id.as_str().to_string())))
    }

    async fn cancel(
        &self,
        args: CancelNotification,
    ) -> agent_client_protocol::Result<()> {
        let key = OurSessionId::new(args.session_id.to_string());
        self.sessions.set_cancelled(key);
        Ok(())
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
            thread_id: Some(entry.thread_id.clone()),
            role_file: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 4096,
            output_json: false,
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
            Err(e) => Err(map_run_error(e)),
        }
    }
}

fn map_run_error(e: RunError) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(e.to_string())
}
