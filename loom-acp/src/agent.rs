//! ACP Agent implementation: maps protocol requests to Loom execution.
//!
//! [`LoomAcpAgent`] implements `agent_client_protocol::Agent` and maps ACP requests
//! to Loom sessions and execution. See [`crate::protocol`] for protocol and behavior details.

use crate::content::content_blocks_to_message;
use crate::session::{SessionId as OurSessionId, SessionStore};
use crate::stream_bridge::{loom_event_to_updates, stream_update_to_session_notification};
use agent_client_protocol::{
    Agent, AuthenticateRequest, AuthenticateResponse, CancelNotification, ContentChunk,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SessionId, SessionNotification, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, StopReason, ToolCall, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields,
};
use loom::memory::{Checkpointer, JsonSerializer, RunnableConfig, SqliteSaver};
use loom::message::Message;
use loom::state::ReActState;

use async_trait::async_trait;
use chrono::DateTime;
use config::load_full_config;
use loom::{run_agent_with_options, AnyStreamEvent, RunCmd, RunCompletion, RunError, RunOptions};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
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

    /// Fetch available models from all configured providers.
    /// Returns a list of ModelOption for the ACP config_options response.
    /// Uses ModelRegistry for caching and unified model access.
    async fn get_available_models(&self) -> Vec<ModelOption> {
        let registry = loom::llm::ModelRegistry::global();

        // Load provider configs from config file
        let providers: Vec<loom::llm::ProviderConfig> = match load_full_config("loom") {
            Ok(config) => config
                .providers
                .into_iter()
                .map(|p| loom::llm::ProviderConfig {
                    name: p.name,
                    base_url: p.base_url,
                    api_key: p.api_key,
                    provider_type: p.provider_type,
                })
                .collect(),
            Err(_) => vec![],
        };

        let entries = registry.list_all_models(&providers).await;

        let all_models: Vec<ModelOption> = entries
            .into_iter()
            .map(|entry| ModelOption {
                // ACP select value + label: "provider/model" so the UI matches registry / RunOptions.
                id: entry.id.clone(),
                name: entry.id,
                provider: entry.provider,
            })
            .collect();

        all_models
    }
}

impl Default for LoomAcpAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl Agent for LoomAcpAgent {
    async fn initialize(
        &self,
        args: InitializeRequest,
    ) -> agent_client_protocol::Result<InitializeResponse> {
        tracing::info!(protocol_version = ?args.protocol_version, "initialize called");
        // Build base response using the standard builder
        let base_response = InitializeResponse::new(args.protocol_version).agent_info(
            agent_client_protocol::Implementation::new("loom", env!("CARGO_PKG_VERSION")),
        );

        // Add loadSession capability by serializing, modifying, and deserializing
        // This is necessary because agent_client_protocol types are non_exhaustive
        let mut json = serde_json::to_value(&base_response)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;

        // Add agentCapabilities with loadSession, sessionCapabilities.list, and promptCapabilities
        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                "agentCapabilities".to_string(),
                serde_json::json!({
                    "loadSession": true,
                    "sessionCapabilities": {
                        "list": {}
                    },
                    "promptCapabilities": {
                        "embeddedContext": true
                    }
                }),
            );
        }

        let response: InitializeResponse = serde_json::from_value(json)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
        tracing::info!("initialize completed");
        Ok(response)
    }

    async fn authenticate(
        &self,
        _args: AuthenticateRequest,
    ) -> agent_client_protocol::Result<AuthenticateResponse> {
        tracing::debug!("authenticate called");
        Ok(AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        args: NewSessionRequest,
    ) -> agent_client_protocol::Result<NewSessionResponse> {
        tracing::debug!(cwd = ?args.cwd, "new_session called");
        // Initialize logging with working_folder from ACP session
        crate::logging::init_with_working_folder(&args.cwd);

        let working_directory = Some(args.cwd.clone());
        let our_id = self.sessions.create(working_directory);
        let session_id = SessionId::new(our_id.as_str().to_string());
        tracing::debug!(session_id = %session_id, "session created");
        let current_model = std::env::var("MODEL")
            .unwrap_or_else(|_| std::env::var("OPENAI_MODEL").unwrap_or_default());
        // Fetch available models from providers
        let model_options = self.get_available_models().await;
        let config_options = build_model_config_options(&current_model, &model_options)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
        Ok(NewSessionResponse::new(session_id).config_options(config_options))
    }

    async fn cancel(&self, args: CancelNotification) -> agent_client_protocol::Result<()> {
        tracing::debug!(session_id = %args.session_id, "cancel called");
        let key = OurSessionId::new(args.session_id.to_string());
        self.sessions.cancel_current_generation(&key);
        Ok(())
    }

    async fn set_session_config_option(
        &self,
        args: SetSessionConfigOptionRequest,
    ) -> agent_client_protocol::Result<SetSessionConfigOptionResponse> {
        tracing::debug!(session_id = %args.session_id, config_id = ?args.config_id, value = ?args.value, "set_session_config_option called");
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
        let model_options = self.get_available_models().await;
        build_set_session_config_option_response(&current_model, &model_options)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))
    }

    async fn prompt(&self, args: PromptRequest) -> agent_client_protocol::Result<PromptResponse> {
        tracing::debug!(session_id = %args.session_id, prompt_blocks = args.prompt.len(), "prompt called");
        let key = OurSessionId::new(args.session_id.to_string());
        let entry = self
            .sessions
            .get(&key)
            .ok_or_else(|| agent_client_protocol::Error::new(-32602, "unknown session"))?;
        let cancellation = self
            .sessions
            .begin_prompt(&key)
            .ok_or_else(|| agent_client_protocol::Error::new(-32602, "unknown session"))?;

        let message = content_blocks_to_message(args.prompt.as_slice()).map_err(|_| {
            agent_client_protocol::Error::new(-32602, "content_blocks parse failed")
        })?;

        tracing::info!(session_id = %args.session_id, message = %message, "User prompt");

        let working_folder = entry
            .working_directory
            .clone()
            .unwrap_or_else(|| PathBuf::from(loom::DEFAULT_WORKING_FOLDER));

        // Load provider configs from config file
        let providers: Vec<loom::llm::ProviderConfig> = match load_full_config("loom") {
            Ok(config) => config
                .providers
                .into_iter()
                .map(|p| loom::llm::ProviderConfig {
                    name: p.name,
                    base_url: p.base_url,
                    api_key: p.api_key,
                    provider_type: p.provider_type,
                })
                .collect(),
            Err(_) => vec![],
        };

        // Parse provider/model format and resolve provider config using ModelRegistry
        let (model, provider_config) = if let Some(ref model_str) = entry.session_config.model {
            // Try to get full model config from ModelRegistry
            if let Some(model_entry) = loom::llm::ModelRegistry::global()
                .get_model(model_str, &providers)
                .await
            {
                (Some(model_entry.name.clone()), Some(model_entry))
            } else if let Some((provider_name, model_id)) = model_str.split_once('/') {
                // Fallback: load provider config directly if not in registry
                // For nested model names like "provider/path/model", use the last segment as the actual model id
                let actual_model_id = model_id
                    .rsplit_once('/')
                    .map(|(_, m)| m)
                    .unwrap_or(model_id);
                tracing::debug!(provider = %provider_name, model_id = %model_id, actual_model_id = %actual_model_id, "Model not in registry, loading provider config");
                let provider_cfg = load_full_config("loom")
                    .ok()
                    .and_then(|c| c.providers.into_iter().find(|p| p.name == provider_name))
                    .map(|p| loom::llm::ModelEntry {
                        id: model_str.clone(),
                        name: actual_model_id.to_string(),
                        provider: p.name,
                        base_url: p.base_url,
                        api_key: p.api_key,
                        provider_type: p.provider_type,
                        ..Default::default()
                    });
                (Some(actual_model_id.to_string()), provider_cfg)
            } else {
                // No provider prefix, use as-is (backward compatibility)
                (Some(model_str.clone()), None)
            }
        } else {
            (None, None)
        };

        let opts = RunOptions {
            message,
            working_folder: Some(working_folder),
            session_id: None,
            cancellation: Some(cancellation.clone()),
            thread_id: Some(entry.thread_id.clone()),
            chat_id: None,
            agent: Some("dev".to_string()),
            verbose: false,
            got_adaptive: false,
            display_max_len: 4096,
            output_json: false,
            model,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
            // Provider config from resolved model entry
            provider: provider_config.as_ref().map(|p| p.provider.clone()),
            base_url: provider_config.as_ref().and_then(|p| p.base_url.clone()),
            api_key: provider_config.as_ref().and_then(|p| p.api_key.clone()),
            provider_type: provider_config
                .as_ref()
                .and_then(|p| p.provider_type.clone()),
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
        tracing::debug!(session_id = %args.session_id, cwd = ?args.cwd, "load_session called");
        let session_id = args.session_id.clone();
        let our_session_id = OurSessionId::new(session_id.to_string());
        let working_directory = Some(args.cwd.clone()); // Convert to Option<PathBuf>

        // Create or get session entry
        let entry =
            if let Some(existing) = self.sessions.get(&our_session_id) {
                existing
            } else {
                // Create new session entry with the provided working directory
                let thread_id = session_id.to_string();
                self.sessions.create_with_id(
                    our_session_id.clone(),
                    working_directory,
                    thread_id.clone(),
                );
                self.sessions.get(&our_session_id).ok_or_else(|| {
                tracing::error!(session_id = %our_session_id, "Session not found after creation");
                agent_client_protocol::Error::internal_error()
                    .data(format!("Session {} not found after creation", our_session_id))
            })?
            };

        // Build checkpointer to load history
        let db_path = loom::memory::default_memory_db_path();
        let serializer = Arc::new(JsonSerializer);
        let checkpointer: Arc<dyn Checkpointer<ReActState>> = Arc::new(
            SqliteSaver::new(db_path.to_string_lossy().as_ref(), serializer).map_err(|e| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("Failed to create checkpointer: {}", e))
            })?,
        );

        // Load checkpoint using thread_id
        let config = RunnableConfig {
            thread_id: Some(entry.thread_id.clone()),
            checkpoint_id: None,
            checkpoint_ns: String::new(),
            user_id: None,
            chat_id: None,
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
                    use std::collections::HashMap;
                    let mut tool_calls_map: HashMap<String, (String, Option<serde_json::Value>)> =
                        HashMap::new();

                    for message in &state.messages {
                        let updates: Vec<SessionNotification> = match message {
                            Message::User(content) => vec![SessionNotification::new(
                                session_id.clone(),
                                agent_client_protocol::SessionUpdate::UserMessageChunk(
                                    ContentChunk::new(content.clone().into()),
                                ),
                            )],
                            Message::Assistant(payload) => {
                                // Cache tool calls for later Tool messages
                                for tc in &payload.tool_calls {
                                    tool_calls_map.insert(
                                        tc.id.clone(),
                                        (tc.name.clone(), serde_json::from_str(&tc.arguments).ok()),
                                    );
                                }

                                // Send assistant message
                                let mut notifications = vec![SessionNotification::new(
                                    session_id.clone(),
                                    agent_client_protocol::SessionUpdate::AgentMessageChunk(
                                        ContentChunk::new(payload.content.clone().into()),
                                    ),
                                )];

                                // Send pending tool calls
                                for tc in &payload.tool_calls {
                                    let tool_call_id = ToolCallId::new(tc.id.clone());
                                    let mut tool_call =
                                        ToolCall::new(tool_call_id.clone(), tc.name.clone())
                                            .status(ToolCallStatus::Pending);
                                    if let Ok(args) =
                                        serde_json::from_str::<serde_json::Value>(&tc.arguments)
                                    {
                                        tool_call = tool_call.raw_input(args);
                                    }
                                    notifications.push(SessionNotification::new(
                                        session_id.clone(),
                                        agent_client_protocol::SessionUpdate::ToolCall(tool_call),
                                    ));
                                }

                                notifications
                            }
                            Message::Tool {
                                tool_call_id,
                                content,
                            } => {
                                // Send ToolCallUpdate with success status
                                let id = ToolCallId::new(tool_call_id.clone());

                                // Convert loom::ToolCallContent to ACP ToolCallContent
                                let acp_content = match content {
                                    loom::tool_source::ToolCallContent::Text(t) => {
                                        agent_client_protocol::ToolCallContent::from(
                                            agent_client_protocol::ContentBlock::Text(
                                                agent_client_protocol::TextContent::new(t.clone()),
                                            ),
                                        )
                                    }
                                    loom::tool_source::ToolCallContent::Diff {
                                        path,
                                        old_text,
                                        new_text,
                                    } => agent_client_protocol::ToolCallContent::Diff(
                                        agent_client_protocol::Diff::new(
                                            path.clone(),
                                            new_text.clone(),
                                        )
                                        .old_text(old_text.clone()),
                                    ),
                                };

                                let fields = ToolCallUpdateFields::new()
                                    .status(ToolCallStatus::Completed)
                                    .content(vec![acp_content]);
                                let tool_call_update = ToolCallUpdate::new(id, fields);

                                vec![SessionNotification::new(
                                    session_id.clone(),
                                    agent_client_protocol::SessionUpdate::ToolCallUpdate(
                                        tool_call_update,
                                    ),
                                )]
                            }
                            Message::System(_) => {
                                // System messages are typically not sent to client
                                continue;
                            }
                        };

                        for update in updates {
                            let _ = tx.try_send(update);
                        }
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

        // Return LoadSessionResponse with config_options
        let current_model = entry.session_config.model.clone().unwrap_or_else(|| {
            std::env::var("MODEL")
                .unwrap_or_else(|_| std::env::var("OPENAI_MODEL").unwrap_or_default())
        });
        let model_options = self.get_available_models().await;
        let config_options = build_model_config_options(&current_model, &model_options)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;

        // Build LoadSessionResponse with configOptions (protocol types are non_exhaustive)
        let json = serde_json::json!({
            "configOptions": config_options,
            "meta": None::<()>
        });
        let response: LoadSessionResponse = serde_json::from_value(json)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
        Ok(response)
    }

    async fn list_sessions(
        &self,
        args: ListSessionsRequest,
    ) -> agent_client_protocol::Result<ListSessionsResponse> {
        tracing::debug!(cwd = ?args.cwd, cursor = ?args.cursor, "list_sessions called");
        // Convert PathBuf cwd to Option<&str> for our internal function
        let cwd_filter = args.cwd.as_ref().and_then(|p| p.to_str());

        // Get sessions from database
        let our_sessions = self
            .list_sessions_from_db(cwd_filter, args.cursor.as_deref())
            .await?;

        // Convert our SessionInfo to JSON and then deserialize to protocol types
        // This is necessary because agent_client_protocol types are non_exhaustive
        let protocol_sessions: Vec<agent_client_protocol::SessionInfo> = our_sessions
            .into_iter()
            .map(|s| {
                // Convert cwd: Option<String> to PathBuf string (use default if None)
                let cwd_str = s
                    .cwd
                    .unwrap_or_else(|| loom::DEFAULT_WORKING_FOLDER.to_string());

                // Build JSON for SessionInfo
                let mut session_json = serde_json::json!({
                    "sessionId": s.session_id,
                    "cwd": cwd_str,
                });

                if let Some(title) = s.title {
                    if let Some(obj) = session_json.as_object_mut() {
                        obj.insert("title".to_string(), serde_json::Value::String(title));
                    }
                }
                if let Some(updated_at) = s.updated_at {
                    if let Some(obj) = session_json.as_object_mut() {
                        obj.insert(
                            "updatedAt".to_string(),
                            serde_json::Value::String(updated_at),
                        );
                    }
                }

                // Convert our SessionMeta to Map<String, Value> for _meta
                if let Some(meta) = s.meta {
                    let mut meta_map = serde_json::Map::new();
                    if let Some(count) = meta.checkpoint_count {
                        meta_map.insert(
                            "checkpoint_count".to_string(),
                            serde_json::Value::Number(count.into()),
                        );
                    }
                    if let Some(count) = meta.message_count {
                        meta_map.insert(
                            "message_count".to_string(),
                            serde_json::Value::Number(count.into()),
                        );
                    }
                    if let Some(step) = meta.latest_step {
                        meta_map.insert(
                            "latest_step".to_string(),
                            serde_json::Value::Number(step.into()),
                        );
                    }
                    if let Some(source) = meta.latest_source {
                        meta_map.insert(
                            "latest_source".to_string(),
                            serde_json::Value::String(source),
                        );
                    }
                    if let Some(obj) = session_json.as_object_mut() {
                        obj.insert("_meta".to_string(), serde_json::Value::Object(meta_map));
                    }
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
            "#
            .to_string();

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
                        Some(format!(
                            "Session {}",
                            &session_id[..8.min(session_id.len())]
                        ))
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

/// Model option for ACP config dropdown.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ModelOption {
    /// Combined id `provider/model` (select value and display; matches [`loom::llm::ModelEntry::id`]).
    id: String,
    /// Same as `id` for the select row label (ACP shows `provider/model`).
    name: String,
    /// Provider name (e.g., "openai", "bigmodel")
    provider: String,
}

/// If `current_model` is a bare model id (e.g. from `MODEL=`) but options use `provider/model`,
/// rewrite to the single matching option when unambiguous.
fn normalize_current_model_for_acp(current_model: &str, options: &[ModelOption]) -> String {
    if current_model.is_empty() {
        return String::new();
    }
    if options.iter().any(|m| m.id == current_model) {
        return current_model.to_string();
    }
    let suffix = format!("/{}", current_model);
    let matches: Vec<_> = options.iter().filter(|m| m.id.ends_with(&suffix)).collect();
    if matches.len() == 1 {
        return matches[0].id.clone();
    }
    current_model.to_string()
}

/// Build config_options array with a single "model" option (protocol types are non_exhaustive, so we construct via serde).
/// SessionConfigOption has kind flattened; SessionConfigKind uses tag "type" → "type": "select" and SessionConfigSelect fields at top level (camelCase).
fn build_model_config_options(
    current_model: &str,
    model_options: &[ModelOption],
) -> Result<Vec<agent_client_protocol::SessionConfigOption>, serde_json::Error> {
    let current_model = normalize_current_model_for_acp(current_model, model_options);
    // Build options array with the correct structure for SessionConfigSelectOptions::Ungrouped
    // Each option needs "value" (not "id") and "name" fields
    // The options field should be an array directly (ungrouped variant is untagged)
    let options: Vec<_> = model_options
        .iter()
        .map(|m| serde_json::json!({ "value": &m.id, "name": &m.name }))
        .collect();

    let json = serde_json::json!([
        {
            "id": "model",
            "name": "Model",
            "description": "LLM model for this session.",
            "category": "model",
            "type": "select",
            "currentValue": current_model,
            "options": options
        }
    ]);
    serde_json::from_value(json)
}

/// Build SetSessionConfigOptionResponse with a single "model" option (protocol types are non_exhaustive, so we construct via serde).
fn build_set_session_config_option_response(
    current_model: &str,
    model_options: &[ModelOption],
) -> Result<SetSessionConfigOptionResponse, serde_json::Error> {
    let config_options = build_model_config_options(current_model, model_options)?;
    let json = serde_json::json!({
        "configOptions": config_options,
        "meta": None::<()>
    });
    serde_json::from_value(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_config_select_option_structure() {
        use agent_client_protocol::{SessionConfigSelectOption, SessionConfigValueId};

        let option_id = SessionConfigValueId::new("gpt-4o".to_string());
        let select_option = SessionConfigSelectOption::new(option_id, "GPT-4o".to_string());

        let json = serde_json::to_value(&select_option).unwrap();
        assert_eq!(json["value"], "gpt-4o");
        assert_eq!(json["name"], "GPT-4o");
    }

    #[test]
    fn test_build_model_config_options_populates_options() {
        let model_options = vec![
            ModelOption {
                id: "openai/gpt-4o".to_string(),
                name: "openai/gpt-4o".to_string(),
                provider: "openai".to_string(),
            },
            ModelOption {
                id: "openai/gpt-4o-mini".to_string(),
                name: "openai/gpt-4o-mini".to_string(),
                provider: "openai".to_string(),
            },
        ];

        // Bare MODEL= id normalizes to the unique provider/model match
        let result = build_model_config_options("gpt-4o", &model_options);
        assert!(result.is_ok(), "Expected Ok, got Err: {:?}", result.err());

        let config_options = result.unwrap();
        assert_eq!(config_options.len(), 1);

        let json = serde_json::to_value(&config_options).unwrap();
        let model_config = &json[0];
        assert_eq!(model_config["id"], "model");
        assert_eq!(model_config["currentValue"], "openai/gpt-4o");

        let options = model_config["options"]
            .as_array()
            .expect("options should be an array");
        assert_eq!(options.len(), 2);
        assert_eq!(options[0]["value"], "openai/gpt-4o");
        assert_eq!(options[0]["name"], "openai/gpt-4o");
    }

    #[test]
    fn test_normalize_current_model_for_acp_ambiguous_bare_id() {
        let model_options = vec![
            ModelOption {
                id: "openai/gpt-4o".to_string(),
                name: "openai/gpt-4o".to_string(),
                provider: "openai".to_string(),
            },
            ModelOption {
                id: "azure/gpt-4o".to_string(),
                name: "azure/gpt-4o".to_string(),
                provider: "azure".to_string(),
            },
        ];
        assert_eq!(
            normalize_current_model_for_acp("gpt-4o", &model_options),
            "gpt-4o"
        );
        assert_eq!(
            normalize_current_model_for_acp("openai/gpt-4o", &model_options),
            "openai/gpt-4o"
        );
    }

    #[test]
    fn test_load_session_response_has_config_options() {
        // Check if LoadSessionResponse supports config_options field
        let response = LoadSessionResponse::default();
        let json = serde_json::to_value(&response).unwrap();
        println!(
            "LoadSessionResponse default JSON: {}",
            serde_json::to_string_pretty(&json).unwrap()
        );

        // Check if configOptions field exists (it should be optional)
        let has_config_options = json.get("configOptions").is_some();
        println!("Has configOptions field: {}", has_config_options);
    }

    #[test]
    fn test_build_model_config_options_handles_empty_list() {
        let result = build_model_config_options("", &[]);
        assert!(result.is_ok(), "Expected Ok, got Err: {:?}", result.err());

        let config_options = result.unwrap();
        let json = serde_json::to_value(&config_options).unwrap();
        let options = json[0]["options"].as_array().unwrap();
        assert!(options.is_empty());
    }

    #[test]
    fn test_model_option_serialization() {
        let option = ModelOption {
            id: "anthropic/claude-3-opus".to_string(),
            name: "anthropic/claude-3-opus".to_string(),
            provider: "anthropic".to_string(),
        };

        let json = serde_json::to_value(&option).unwrap();
        assert_eq!(json["id"], "anthropic/claude-3-opus");
        assert_eq!(json["name"], "anthropic/claude-3-opus");
    }

    #[test]
    fn test_build_set_session_config_option_response() {
        let model_options = vec![ModelOption {
            id: "openai/gpt-4o".to_string(),
            name: "openai/gpt-4o".to_string(),
            provider: "openai".to_string(),
        }];

        let result = build_set_session_config_option_response("gpt-4o", &model_options);
        assert!(result.is_ok());

        let response = result.unwrap();
        let json = serde_json::to_value(&response).unwrap();
        assert!(json["configOptions"].is_array());
    }
}
