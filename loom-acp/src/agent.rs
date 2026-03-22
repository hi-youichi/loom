//! ACP Agent implementation: maps protocol requests to Loom execution.
//!
//! [`LoomAcpAgent`] implements `agent_client_protocol::Agent` and maps ACP requests
//! to Loom sessions and execution. See [`crate::protocol`] for protocol and behavior details.

use crate::content::content_blocks_to_message;
use crate::session::{SessionId as OurSessionId, SessionStore};
use crate::stream_bridge::{loom_event_to_updates, stream_update_to_session_notification};
use agent_client_protocol::{
    Agent, AuthenticateRequest, AuthenticateResponse, CancelNotification,
    InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse,
    ListSessionsRequest, ListSessionsResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SessionId, SessionNotification, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, StopReason, ContentChunk,
};
use loom::memory::{Checkpointer, RunnableConfig, JsonSerializer, SqliteSaver};
use loom::state::ReActState;
use loom::message::Message;
use std::sync::Arc;
use async_trait::async_trait;
use loom::{run_agent_with_options, AnyStreamEvent, RunCmd, RunCompletion, RunError, RunOptions};
use std::path::PathBuf;
use tokio::sync::mpsc;
use rusqlite::Connection;
use chrono::DateTime;

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
        // Build base response using the standard builder
        let base_response = InitializeResponse::new(args.protocol_version)
            .agent_info(agent_client_protocol::Implementation::new("loom", env!("CARGO_PKG_VERSION")));
        
        // Add loadSession capability by serializing, modifying, and deserializing
        // This is necessary because agent_client_protocol types are non_exhaustive
        let mut json = serde_json::to_value(&base_response)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
        
        // Add agentCapabilities with loadSession and sessionCapabilities.list
        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                "agentCapabilities".to_string(),
                serde_json::json!({
                    "loadSession": true,
                    "sessionCapabilities": {
                        "list": {}
                    }
                })
            );
        }
        
        serde_json::from_value(json)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))
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
        self.sessions.cancel_current_generation(&key);
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
        let cancellation = self
            .sessions
            .begin_prompt(&key)
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
            cancellation: Some(cancellation.clone()),
            thread_id: Some(entry.thread_id.clone()),
            role_file: None,
            agent: Some("dev".to_string()),
            verbose: false,
            got_adaptive: false,
            display_max_len: 4096,
            output_json: false,
            model: entry.session_config.model.clone(),
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
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

        let result = run_agent_with_options(&opts, &RunCmd::React, on_event).await;
        self.sessions.finish_prompt(&key, cancellation.generation());
        match result {
            Ok(RunCompletion::Finished(_reply)) => Ok(PromptResponse::new(StopReason::EndTurn)),
            Ok(RunCompletion::Cancelled) => Ok(PromptResponse::new(StopReason::Cancelled)),
            Err(e) => {
                tracing::error!(session_id = %args.session_id, error = %e, "run_agent failed");
                Err(map_run_error(e))
            }
        }
    }

    async fn load_session(
        &self,
        args: LoadSessionRequest,
    ) -> agent_client_protocol::Result<LoadSessionResponse> {
        let session_id = args.session_id.clone();
        let our_session_id = OurSessionId::new(session_id.to_string());
        let working_directory = Some(args.cwd.clone()); // Convert to Option<PathBuf>
        
        // Create or get session entry
        let entry = if let Some(existing) = self.sessions.get(&our_session_id) {
            existing
        } else {
            // Create new session entry with the provided working directory
            let thread_id = session_id.to_string();
            self.sessions.create_with_id(our_session_id.clone(), working_directory, thread_id.clone());
            self.sessions.get(&our_session_id).expect("Session should exist after create_with_id")
        };

        // Build checkpointer to load history
        let db_path = loom::memory::default_memory_db_path();
        let serializer = Arc::new(JsonSerializer);
        let checkpointer: Arc<dyn Checkpointer<ReActState>> = Arc::new(
            SqliteSaver::new(
                db_path.to_string_lossy().as_ref(),
                serializer,
            ).map_err(|e| agent_client_protocol::Error::internal_error()
                .data(format!("Failed to create checkpointer: {}", e)))?
        );

        // Load checkpoint using thread_id
        let config = RunnableConfig {
            thread_id: Some(entry.thread_id.clone()),
            checkpoint_id: None,
            checkpoint_ns: String::new(),
            user_id: None,
            resume_from_node_id: None,
            depth: None,
            resume_value: None,
            resume_values_by_namespace: Default::default(),
            resume_values_by_interrupt_id: Default::default(),
        };

        // Try to load checkpoint
        match checkpointer.get_tuple(&config).await {
            Ok(Some((checkpoint, _metadata))) => {
                // Extract messages from state
                let state: ReActState = checkpoint.channel_values;
                
                // Send history via session/update notifications
                if let Some(ref tx) = self.session_update_tx {
                    for message in &state.messages {
                        let update = match message {
                            Message::User(content) => {
                                SessionNotification::new(
                                    session_id.clone(),
                                    agent_client_protocol::SessionUpdate::UserMessageChunk(
                                        ContentChunk::new(content.clone().into())
                                    )
                                )
                            }
                            Message::Assistant(content) => {
                                SessionNotification::new(
                                    session_id.clone(),
                                    agent_client_protocol::SessionUpdate::AgentMessageChunk(
                                        ContentChunk::new(content.clone().into())
                                    )
                                )
                            }
                            Message::System(_) => {
                                // System messages are typically not sent to client
                                continue;
                            }
                        };
                        let _ = tx.try_send(update);
                    }
                }
                
                tracing::info!(
                    session_id = %session_id,
                    message_count = state.messages.len(),
                    "Loaded and replayed session history"
                );
            }
            Ok(None) => {
                tracing::debug!(
                    session_id = %session_id,
                    "No checkpoint found for session, starting fresh"
                );
            }
            Err(e) => {
                tracing::warn!(
                    session_id = %session_id,
                    error = %e,
                    "Failed to load checkpoint, starting fresh"
                );
                // Continue without error - session can start fresh
            }
        }

        // TODO: Connect MCP servers from request
        // This would require storing MCP connections per session
        // For now, we just log that they were requested
        if !args.mcp_servers.is_empty() {
            tracing::debug!(
                session_id = %session_id,
                mcp_server_count = args.mcp_servers.len(),
                "MCP servers requested for loaded session"
            );
        }

        // Return LoadSessionResponse
        Ok(LoadSessionResponse::default())
    }

    async fn list_sessions(
        &self,
        args: ListSessionsRequest,
    ) -> agent_client_protocol::Result<ListSessionsResponse> {
        // Convert PathBuf cwd to Option<&str> for our internal function
        let cwd_filter = args.cwd.as_ref()
            .and_then(|p| p.to_str());
        
        // Get sessions from database
        let our_sessions = self.list_sessions_from_db(cwd_filter, args.cursor.as_deref()).await?;
        
        // Convert our SessionInfo to JSON and then deserialize to protocol types
        // This is necessary because agent_client_protocol types are non_exhaustive
        let protocol_sessions: Vec<agent_client_protocol::SessionInfo> = our_sessions
            .into_iter()
            .map(|s| {
                // Convert cwd: Option<String> to PathBuf string (use default if None)
                let cwd_str = s.cwd
                    .unwrap_or_else(|| loom::DEFAULT_WORKING_FOLDER.to_string());
                
                // Build JSON for SessionInfo
                let mut session_json = serde_json::json!({
                    "sessionId": s.session_id,
                    "cwd": cwd_str,
                });
                
                if let Some(title) = s.title {
                    session_json.as_object_mut().unwrap().insert("title".to_string(), serde_json::Value::String(title));
                }
                if let Some(updated_at) = s.updated_at {
                    session_json.as_object_mut().unwrap().insert("updatedAt".to_string(), serde_json::Value::String(updated_at));
                }
                
                // Convert our SessionMeta to Map<String, Value> for _meta
                if let Some(meta) = s.meta {
                    let mut meta_map = serde_json::Map::new();
                    if let Some(count) = meta.checkpoint_count {
                        meta_map.insert("checkpoint_count".to_string(), serde_json::Value::Number(count.into()));
                    }
                    if let Some(count) = meta.message_count {
                        meta_map.insert("message_count".to_string(), serde_json::Value::Number(count.into()));
                    }
                    if let Some(step) = meta.latest_step {
                        meta_map.insert("latest_step".to_string(), serde_json::Value::Number(step.into()));
                    }
                    if let Some(source) = meta.latest_source {
                        meta_map.insert("latest_source".to_string(), serde_json::Value::String(source));
                    }
                    session_json.as_object_mut().unwrap().insert("_meta".to_string(), serde_json::Value::Object(meta_map));
                }
                
                serde_json::from_value(session_json)
                    .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        
        // Build response JSON (pagination not implemented yet, so next_cursor is None)
        let response_json = serde_json::json!({
            "sessions": protocol_sessions,
            "nextCursor": None::<()>,
        });
        
        serde_json::from_value(response_json)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))
    }
}

/// Session information for ACP session/list response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    /// Session ID (thread_id)
    pub session_id: String,
    /// Working directory (cwd) for the session
    pub cwd: Option<String>,
    /// Human-readable title for the session (auto-generated from first prompt or summary)
    pub title: Option<String>,
    /// ISO 8601 timestamp of the last activity
    pub updated_at: Option<String>,
    /// Agent-specific metadata
    #[serde(rename = "_meta")]
    pub meta: Option<SessionMeta>,
}

/// Agent-specific metadata for a session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionMeta {
    /// Number of checkpoints in this session
    pub checkpoint_count: Option<usize>,
    /// Number of messages in the session
    pub message_count: Option<usize>,
    /// Latest step number
    pub latest_step: Option<i64>,
    /// Source of the latest checkpoint
    pub latest_source: Option<String>,
}

impl LoomAcpAgent {
    /// Lists all sessions from the SQLite database.
    /// 
    /// This function queries the checkpoints table to find all unique thread_ids
    /// and returns session information including metadata.
    pub async fn list_sessions_from_db(
        &self,
        cwd_filter: Option<&str>,
        _cursor: Option<&str>,
    ) -> Result<Vec<SessionInfo>, agent_client_protocol::Error> {
        let db_path = loom::memory::default_memory_db_path();
        let cwd_filter = cwd_filter.map(String::from);
        
        // Use spawn_blocking for SQLite operations
        let sessions = tokio::task::spawn_blocking(move || -> Result<Vec<SessionInfo>, String> {
            let conn = Connection::open(&db_path)
                .map_err(|e| format!("Failed to open database: {}", e))?;

            // Build query with optional cwd filter
            let mut sql = r#"
                SELECT 
                    thread_id,
                    COUNT(*) as checkpoint_count,
                    MIN(metadata_created_at) as created_at,
                    MAX(metadata_created_at) as last_updated,
                    (SELECT metadata_step FROM checkpoints c2 
                     WHERE c2.thread_id = c1.thread_id 
                     ORDER BY metadata_created_at DESC LIMIT 1) as latest_step,
                    (SELECT metadata_source FROM checkpoints c2 
                     WHERE c2.thread_id = c1.thread_id 
                     ORDER BY metadata_created_at DESC LIMIT 1) as latest_source,
                    (SELECT metadata_summary FROM checkpoints c2 
                     WHERE c2.thread_id = c1.thread_id 
                     ORDER BY metadata_created_at DESC LIMIT 1) as latest_summary
                FROM checkpoints c1
            "#.to_string();

            // Note: We don't store cwd in checkpoints table directly,
            // so we can't filter by cwd yet. This would require storing
            // cwd in the checkpoints table or a separate sessions table.
            // For now, we'll return all sessions regardless of cwd_filter.
            let _ = cwd_filter;

            sql.push_str(" GROUP BY thread_id ORDER BY last_updated DESC");

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| format!("Failed to prepare statement: {}", e))?;

            let sessions = stmt
                .query_map([], |row: &rusqlite::Row| {
                    let session_id: String = row.get(0)?;
                    let checkpoint_count: usize = row.get(1)?;
                    let _created_at_ms: Option<i64> = row.get(2)?;
                    let last_updated_ms: Option<i64> = row.get(3)?;
                    let latest_step: i64 = row.get(4)?;
                    let latest_source: String = row.get(5)?;
                    let latest_summary: Option<String> = row.get(6)?;

                    let updated_at = last_updated_ms
                        .and_then(|ms| DateTime::from_timestamp_millis(ms))
                        .map(|dt| dt.to_rfc3339());

                    // Use summary as title if available, otherwise generate from session_id
                    let title = latest_summary.or_else(|| {
                        Some(format!("Session {}", &session_id[..8.min(session_id.len())]))
                    });

                    Ok(SessionInfo {
                        session_id,
                        cwd: None, // TODO: Store cwd in checkpoints or separate table
                        title,
                        updated_at,
                        meta: Some(SessionMeta {
                            checkpoint_count: Some(checkpoint_count),
                            message_count: None, // Would need to deserialize checkpoint to get message count
                            latest_step: Some(latest_step),
                            latest_source: Some(latest_source),
                        }),
                    })
                })
                .map_err(|e| format!("Failed to query sessions: {}", e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to collect sessions: {}", e))?;

            Ok(sessions)
        })
        .await
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e))?;

        Ok(sessions)
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
