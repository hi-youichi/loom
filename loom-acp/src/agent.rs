//! ACP Agent implementation: maps protocol requests to Loom execution.
//!
//! [`LoomAcpAgent`] implements `agent_client_protocol::Agent` and maps ACP requests
//! to Loom sessions and execution. See [`crate::protocol`] for protocol and behavior details.

use crate::agent_registry::AgentRegistry;
use crate::content::{content_blocks_to_user_content, extract_locations};
use crate::session::{SessionId as OurSessionId, SessionStore};
use crate::session_config_store::SessionConfigStore;
use crate::stream_bridge::{loom_event_to_updates, stream_update_to_session_notification, generate_tool_title, name_to_tool_kind};
use agent_client_protocol::{
    Agent, AuthenticateRequest, AuthenticateResponse, CancelNotification, ContentChunk,
    CurrentModeUpdate, ForkSessionRequest, ForkSessionResponse, InitializeRequest,
    InitializeResponse, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    SessionId, SessionModeId, SessionConfigOptionValue, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, SetSessionModelRequest, SetSessionModelResponse, StopReason,
    Terminal, TerminalId, ToolCall, ToolCallId, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields,
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
    pub(crate) agent_registry: AgentRegistry,
    pub(crate) config_store: SessionConfigStore,
    /// If Some, on_event during prompt converts stream events to SessionNotification and try_sends here.
    pub(crate) session_update_tx: Option<mpsc::Sender<SessionNotification>>,
}

impl LoomAcpAgent {
    /// Construct a new Agent instance (no session/update sending).
    pub fn new() -> Self {
        let db_path = loom::memory::default_memory_db_path();
        let config_store = SessionConfigStore::new(db_path.to_str().unwrap_or_default())
            .expect("Failed to initialize session config store");
        
        Self {
            sessions: SessionStore::new(),
            agent_registry: AgentRegistry::new(),
            config_store,
            session_update_tx: None,
        }
    }

    /// Construct an Agent with a session/update sender for the stdio loop to push stream updates to the client.
    pub fn with_session_update_tx(tx: mpsc::Sender<SessionNotification>) -> Self {
        let db_path = loom::memory::default_memory_db_path();
        let config_store = SessionConfigStore::new(db_path.to_str().unwrap_or_default())
            .expect("Failed to initialize session config store");
        
        Self {
            sessions: SessionStore::new(),
            agent_registry: AgentRegistry::new(),
            config_store,
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
                    fetch_models: p.fetch_models.unwrap_or(false),
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

    fn apply_session_mode(
        &self,
        session_id: &SessionId,
        key: &OurSessionId,
        mode_id: &str,
    ) -> agent_client_protocol::Result<()> {
        if self.sessions.get(key).is_none() {
            return Err(agent_client_protocol::Error::new(-32602, "unknown session"));
        }

        if !self.agent_registry.mode_exists(mode_id) {
            return Err(agent_client_protocol::Error::new(
                -32602,
                format!("unknown mode: {}", mode_id),
            ));
        }

        self.sessions.update_session_config(key, |c| {
            c.current_agent = mode_id.to_string();
        });

        if let Some(tx) = &self.session_update_tx {
            let notif = SessionNotification::new(
                session_id.clone(),
                SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(SessionModeId::new(mode_id))),
            );
            let _ = tx.try_send(notif);
        }

        Ok(())
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
                        "list": {},
                        "fork": {}
                    },
                    "promptCapabilities": {
                        "embeddedContext": true,
                        "image": true,
                        "audio": true
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

        let default_mode = self.agent_registry.default_mode_id();
        let current_model = std::env::var("MODEL")
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(crate::last_model::load)
            .unwrap_or_default();
        self.sessions.update_session_config(&our_id, |c| {
            c.current_agent = default_mode.to_string();
            if !current_model.is_empty() {
                c.model = Some(current_model.clone());
            }
        });
        if !current_model.is_empty() {
            if let Err(e) = self.config_store.set(&our_id, "model", &current_model) {
                tracing::warn!(session_id = %our_id, error = %e, "Failed to persist initial model config");
            }
        }
        let model_options = self.get_available_models().await;
        let current_mode = default_mode;
        let modes = self.agent_registry.to_session_modes();
        let config_options =
            build_session_config_options(current_mode, &current_model, &modes, &model_options)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
        Ok(NewSessionResponse::new(session_id)
            .modes(self.agent_registry.to_session_mode_state(current_mode))
            .config_options(config_options))
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
        let value_str = session_config_value_as_id(&args.value).ok_or_else(|| {
            agent_client_protocol::Error::new(
                -32602,
                format!(
                    "unsupported value type for config_id {}: expected select value id",
                    config_id_str
                ),
            )
        })?;
        match config_id_str.as_str() {
            "model" => {
                self.sessions
                    .update_session_config(&key, |c| c.model = Some(value_str.clone()));
                crate::last_model::save(&value_str);
                // Persist to database
                if let Err(e) = self.config_store.set(&key, "model", &value_str) {
                    tracing::warn!(session_id = %args.session_id, error = %e, "Failed to persist model config");
                }
            }
            "mode" => {
                self.apply_session_mode(&args.session_id, &key, &value_str)?;
                // Persist to database
                if let Err(e) = self.config_store.set(&key, "mode", &value_str) {
                    tracing::warn!(session_id = %args.session_id, error = %e, "Failed to persist mode config");
                }
            }
            _ => {
                return Err(agent_client_protocol::Error::new(
                    -32602,
                    format!("unsupported config_id: {}", config_id_str),
                ));
            }
        }

        let entry = self
            .sessions
            .get(&key)
            .ok_or_else(|| agent_client_protocol::Error::new(-32602, "unknown session"))?;
        let current_mode = if entry.session_config.current_agent.is_empty() {
            self.agent_registry.default_mode_id().to_string()
        } else {
            entry.session_config.current_agent.clone()
        };
        let current_model = entry.session_config.model.clone().unwrap_or_else(|| {
            std::env::var("MODEL")
                .or_else(|_| std::env::var("OPENAI_MODEL"))
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(crate::last_model::load)
                .unwrap_or_default()
        });
        let modes = self.agent_registry.to_session_modes();
        let model_options = self.get_available_models().await;
        build_set_session_config_option_response(&current_mode, &current_model, &modes, &model_options)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))
    }

    async fn set_session_mode(
        &self,
        args: SetSessionModeRequest,
    ) -> agent_client_protocol::Result<SetSessionModeResponse> {
        let mode_id = args.mode_id.to_string();
        tracing::debug!(session_id = %args.session_id, mode_id = %mode_id, "set_session_mode called");

        let key = OurSessionId::new(args.session_id.to_string());
        self.apply_session_mode(&args.session_id, &key, &mode_id)?;
        
        // Persist to database
        if let Err(e) = self.config_store.set(&key, "mode", &mode_id) {
            tracing::warn!(session_id = %args.session_id, error = %e, "Failed to persist mode config");
        }
        
        Ok(SetSessionModeResponse::new())
    }

    async fn set_session_model(
        &self,
        args: SetSessionModelRequest,
    ) -> agent_client_protocol::Result<SetSessionModelResponse> {
        let model_id = args.model_id.to_string();
        tracing::debug!(session_id = %args.session_id, model_id = %model_id, "set_session_model called");

        let key = OurSessionId::new(args.session_id.to_string());
        if self.sessions.get(&key).is_none() {
            return Err(agent_client_protocol::Error::new(-32602, "unknown session"));
        }

        // Update the model in session config
        self.sessions
            .update_session_config(&key, |c| c.model = Some(model_id.clone()));
        crate::last_model::save(&model_id);
        
        // Persist to database
        if let Err(e) = self.config_store.set(&key, "model", &model_id) {
            tracing::warn!(session_id = %args.session_id, error = %e, "Failed to persist model config");
        }

        Ok(SetSessionModelResponse::new())
    }

    async fn fork_session(
        &self,
        args: ForkSessionRequest,
    ) -> agent_client_protocol::Result<ForkSessionResponse> {
        tracing::debug!(session_id = %args.session_id, cwd = ?args.cwd, "fork_session called");
        crate::logging::init_with_working_folder(&args.cwd);

        let source_key = OurSessionId::new(args.session_id.to_string());
        let source_entry = self.sessions.get(&source_key).ok_or_else(|| {
            agent_client_protocol::Error::new(-32602, "unknown session")
        })?;

        // Create new session with the same working directory and config
        let new_our_id = self.sessions.create(source_entry.working_directory.clone());
        let new_session_id = SessionId::new(new_our_id.as_str().to_string());

        // Copy source session config (model, mode) to the new session
        self.sessions.update_session_config(&new_our_id, |c| {
            *c = source_entry.session_config.clone();
        });
        
        // Copy persistent config from source to target
        if let Err(e) = self.config_store.copy_config(&source_key, &new_our_id) {
            tracing::warn!(
                source_session = %args.session_id,
                target_session = %new_session_id,
                error = %e,
                "Failed to copy persistent config during fork"
            );
        }
        tracing::info!(source_session = %args.session_id, new_session = %new_session_id, "session forked");

        let current_mode = if source_entry.session_config.current_agent.is_empty() {
            self.agent_registry.default_mode_id().to_string()
        } else {
            source_entry.session_config.current_agent.clone()
        };
        let current_model = source_entry.session_config.model.clone().unwrap_or_else(|| {
            std::env::var("MODEL")
                .or_else(|_| std::env::var("OPENAI_MODEL"))
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(crate::last_model::load)
                .unwrap_or_default()
        });
        // If model was resolved from fallback (env/last_model) rather than source config, persist it
        if !current_model.is_empty() && source_entry.session_config.model.is_none() {
            self.sessions.update_session_config(&new_our_id, |c| {
                c.model = Some(current_model.clone());
            });
            if let Err(e) = self.config_store.set(&new_our_id, "model", &current_model) {
                tracing::warn!(
                    session_id = %new_session_id,
                    error = %e,
                    "Failed to persist initial model config in forked session"
                );
            }
        }

        let model_options = self.get_available_models().await;
        let modes = self.agent_registry.to_session_modes();
        let config_options = build_session_config_options(
            &current_mode,
            &current_model,
            &modes,
            &model_options,
        )
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;

        Ok(ForkSessionResponse::new(new_session_id)
            .modes(self.agent_registry.to_session_mode_state(&current_mode))
            .config_options(config_options))
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

        let user_content = content_blocks_to_user_content(args.prompt.as_slice()).map_err(|_| {
            agent_client_protocol::Error::new(-32602, "content_blocks parse failed")
        })?;

        if let loom::message::UserContent::Text(ref text) = user_content {
            if let Some(cmd) = loom::command::parse(text) {
                match cmd {
                    loom::command::Command::ResetContext => {
                        self.sessions.cancel_current_generation(&key);
                        tracing::info!(session_id = %args.session_id, "Context cleared via /reset command");
                        return Ok(PromptResponse::new(StopReason::EndTurn));
                    }
                    loom::command::Command::Models { .. } | loom::command::Command::ModelsUse { .. } => {
                        // ACP handles models via SetSessionConfigOption, not here
                    }
                    _ => {
                        return Ok(PromptResponse::new(StopReason::EndTurn));
                    }
                }
            }
        }

        let content_type = match &user_content {
            loom::message::UserContent::Text(_) => "text",
            loom::message::UserContent::Multimodal(parts) => {
                let has_image = parts
                    .iter()
                    .any(|p| matches!(p, loom::message::ContentPart::ImageBase64 { .. }));
                let has_audio = parts
                    .iter()
                    .any(|p| matches!(p, loom::message::ContentPart::AudioBase64 { .. }));
                if has_image && has_audio {
                    "multimodal(image+audio)"
                } else if has_image {
                    "multimodal(image)"
                } else if has_audio {
                    "multimodal(audio)"
                } else {
                    "multimodal"
                }
            }
        };
        tracing::info!(
            session_id = %args.session_id,
            content_type = content_type,
            text_len = user_content.as_text().len(),
            "User prompt"
        );

        let working_folder = entry
            .working_directory
            .clone()
            .unwrap_or_else(|| PathBuf::from(loom::DEFAULT_WORKING_FOLDER));

        let resolved = loom::resolve_model_config(
            entry.session_config.model.as_deref(),
        )
        .await;

        let opts = RunOptions {
            message: user_content,
            working_folder: Some(working_folder),
            session_id: None,
            cancellation: Some(cancellation.clone()),
            thread_id: Some(entry.thread_id.clone()),
            agent: Some(self.agent_registry.resolve_agent_name(&entry.session_config.current_agent)),
            verbose: false,
            got_adaptive: false,
            display_max_len: 4096,
            output_json: false,
            model: resolved.model,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
            provider: resolved.provider,
            base_url: resolved.base_url,
            api_key: resolved.api_key,
            provider_type: resolved.provider_type,
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
        // Initialize logging with working_folder from ACP session
        crate::logging::init_with_working_folder(&args.cwd);
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
                let default_mode = self.agent_registry.default_mode_id();
                self.sessions.update_session_config(&our_session_id, |c| {
                    if c.current_agent.is_empty() {
                        c.current_agent = default_mode.to_string();
                    }
                });
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
                                    ContentChunk::new(agent_client_protocol::ContentBlock::Text(
                                        agent_client_protocol::TextContent::new(content.as_text().to_string()),
                                    )),
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
                                    let args = serde_json::from_str::<serde_json::Value>(&tc.arguments).ok();
                                    let title = generate_tool_title(&tc.name, args.as_ref());
                                    let kind = name_to_tool_kind(&tc.name);
                                    let mut tool_call = ToolCall::new(tool_call_id.clone(), title)
                                        .status(ToolCallStatus::Pending)
                                        .kind(kind);
                                    if let Some(ref args) = args {
                                        tool_call = tool_call.raw_input(args.clone());
                                        // Add locations for "follow-along" feature
                                        let locations: Vec<ToolCallLocation> = extract_locations(&tc.name, args)
                                            .into_iter()
                                            .map(|loc| ToolCallLocation::new(loc.path).line(loc.line))
                                            .collect();
                                        if !locations.is_empty() {
                                            tool_call = tool_call.locations(locations);
                                        }
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
                                    loom::tool_source::ToolCallContent::Terminal { terminal_id } => {
                                        agent_client_protocol::ToolCallContent::Terminal(
                                            Terminal::new(TerminalId::new(terminal_id.clone())),
                                        )
                                    }
                                };

                                let fields = ToolCallUpdateFields::new()
                                    .status(ToolCallStatus::Completed)
                                    .content(vec![acp_content])
                                    .raw_output(tool_call_content_to_raw_output(content));
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
                            if let Err(e) = tx.send(update).await {
                                tracing::error!(session_id = %session_id, error = %e, "Failed to send session update during history replay");
                            }
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

        // Return LoadSessionResponse with config_options and modes
        // First, try to load from persistent store
        let persisted_config = self.config_store.get_all(&our_session_id)
            .unwrap_or_else(|e| {
                tracing::warn!(session_id = %session_id, error = %e, "Failed to load persistent config");
                std::collections::HashMap::new()
            });
        
        let current_mode = persisted_config.get("mode")
            .cloned()
            .unwrap_or_else(|| {
                if entry.session_config.current_agent.is_empty() {
                    self.agent_registry.default_mode_id().to_string()
                } else {
                    entry.session_config.current_agent.clone()
                }
            });
        
        let current_model = persisted_config.get("model")
            .cloned()
            .unwrap_or_else(|| {
                entry.session_config.model.clone().unwrap_or_else(|| {
                    std::env::var("MODEL")
                        .or_else(|_| std::env::var("OPENAI_MODEL"))
                        .ok()
                        .filter(|s| !s.is_empty())
                        .or_else(crate::last_model::load)
                        .unwrap_or_default()
                })
            });
        let model_options = self.get_available_models().await;
        let available_modes = self.agent_registry.to_session_modes();
        let config_options = build_session_config_options(
            &current_mode,
            &current_model,
            &available_modes,
            &model_options,
        )
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;

        let modes = self.agent_registry.to_session_mode_state(&current_mode);

        // Build LoadSessionResponse with configOptions and modes (protocol types are non_exhaustive)
        let json = serde_json::json!({
            "configOptions": config_options,
            "modes": modes,
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

            // Build query: use CTE to get latest checkpoint per thread,
            // then join with aggregate stats. This avoids 3 correlated subqueries.
            let mut sql = r#"
                SELECT 
                    c.thread_id,
                    c.checkpoint_count,
                    c.created_at,
                    c.last_updated,
                    lc.metadata_step as latest_step,
                    lc.metadata_source as latest_source,
                    lc.metadata_summary as latest_summary
                FROM (
                    SELECT 
                        thread_id,
                        COUNT(*) as checkpoint_count,
                        MIN(metadata_created_at) as created_at,
                        MAX(metadata_created_at) as last_updated
                    FROM checkpoints
                    GROUP BY thread_id
                ) c
                INNER JOIN checkpoints lc ON lc.thread_id = c.thread_id
                    AND lc.metadata_created_at = c.last_updated
                "#
            .to_string();

            // Note: We don't store cwd in checkpoints table directly,
            // so we can't filter by cwd yet. This would require storing
            // cwd in the checkpoints table or a separate sessions table.
            // For now, we'll return all sessions regardless of cwd_filter.
            let _ = cwd_filter;

            sql.push_str(" ORDER BY COALESCE(c.last_updated, 0) DESC");

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

                    // Use summary as title if available and not empty, otherwise generate from session_id
                    let title = latest_summary
                        .filter(|s| !s.trim().is_empty())
                        .or_else(|| {
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

fn tool_call_content_to_raw_output(content: &loom::tool_source::ToolCallContent) -> serde_json::Value {
    match content {
        loom::tool_source::ToolCallContent::Text(text) => {
            // For text content (like bash output), return as string directly
            // This preserves the original output format without JSON parsing attempts
            serde_json::json!(text)
        }
        loom::tool_source::ToolCallContent::Diff {
            path,
            old_text,
            new_text,
        } => serde_json::json!({
            "type": "diff",
            "path": path,
            "oldText": old_text,
            "newText": new_text,
        }),
        loom::tool_source::ToolCallContent::Terminal { terminal_id } => serde_json::json!({
            "type": "terminal",
            "terminalId": terminal_id,
        }),
    }
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

/// Build config_options array with "mode" and "model" options (protocol types are non_exhaustive, so we construct via serde).
/// SessionConfigOption has kind flattened; SessionConfigKind uses tag "type" → "type": "select" and SessionConfigSelect fields at top level (camelCase).
fn build_session_config_options(
    current_mode: &str,
    current_model: &str,
    modes: &[agent_client_protocol::SessionMode],
    model_options: &[ModelOption],
) -> Result<Vec<agent_client_protocol::SessionConfigOption>, serde_json::Error> {
    let current_model = normalize_current_model_for_acp(current_model, model_options);
    let mode_options: Vec<_> = modes
        .iter()
        .map(|m| {
            serde_json::json!({
                "value": m.id.to_string(),
                "name": m.name.to_string()
            })
        })
        .collect();
    let model_options: Vec<_> = model_options
        .iter()
        .map(|m| serde_json::json!({ "value": &m.id, "name": &m.name }))
        .collect();

    let json = serde_json::json!([
        {
            "id": "mode",
            "name": "Mode",
            "description": "Session behavior mode.",
            "category": "mode",
            "type": "select",
            "currentValue": current_mode,
            "options": mode_options
        },
        {
            "id": "model",
            "name": "Model",
            "description": "LLM model for this session.",
            "category": "model",
            "type": "select",
            "currentValue": current_model,
            "options": model_options
        }
    ]);
    serde_json::from_value(json)
}

/// Build SetSessionConfigOptionResponse with a single "model" option (protocol types are non_exhaustive, so we construct via serde).
fn build_set_session_config_option_response(
    current_mode: &str,
    current_model: &str,
    modes: &[agent_client_protocol::SessionMode],
    model_options: &[ModelOption],
) -> Result<SetSessionConfigOptionResponse, serde_json::Error> {
    let config_options =
        build_session_config_options(current_mode, current_model, modes, model_options)?;
    let json = serde_json::json!({
        "configOptions": config_options,
        "meta": None::<()>
    });
    serde_json::from_value(json)
}

fn session_config_value_as_id(value: &SessionConfigOptionValue) -> Option<String> {
    match value {
        SessionConfigOptionValue::ValueId { value } => Some(value.to_string()),
        SessionConfigOptionValue::Boolean { .. } => None,
        _ => None,
    }
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
    fn test_build_session_config_options_populates_options() {
        let modes = vec![
            agent_client_protocol::SessionMode::new(
                agent_client_protocol::SessionModeId::new("ask"),
                "Ask",
            ),
            agent_client_protocol::SessionMode::new(
                agent_client_protocol::SessionModeId::new("default"),
                "Default",
            ),
        ];
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
        let result = build_session_config_options("ask", "gpt-4o", &modes, &model_options);
        assert!(result.is_ok(), "Expected Ok, got Err: {:?}", result.err());

        let config_options = result.unwrap();
        assert_eq!(config_options.len(), 2);

        let json = serde_json::to_value(&config_options).unwrap();
        assert_eq!(json[0]["id"], "mode");
        assert_eq!(json[0]["category"], "mode");
        assert_eq!(json[0]["currentValue"], "ask");
        assert_eq!(json[1]["id"], "model");
        let model_config = &json[1];
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
    fn test_build_session_config_options_handles_empty_model_list() {
        let modes = vec![agent_client_protocol::SessionMode::new(
            agent_client_protocol::SessionModeId::new("ask"),
            "Ask",
        )];
        let result = build_session_config_options("ask", "", &modes, &[]);
        assert!(result.is_ok(), "Expected Ok, got Err: {:?}", result.err());

        let config_options = result.unwrap();
        let json = serde_json::to_value(&config_options).unwrap();
        let options = json[1]["options"].as_array().unwrap();
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
        let modes = vec![agent_client_protocol::SessionMode::new(
            agent_client_protocol::SessionModeId::new("ask"),
            "Ask",
        )];
        let model_options = vec![ModelOption {
            id: "openai/gpt-4o".to_string(),
            name: "openai/gpt-4o".to_string(),
            provider: "openai".to_string(),
        }];

        let result =
            build_set_session_config_option_response("ask", "gpt-4o", &modes, &model_options);
        assert!(result.is_ok());

        let response = result.unwrap();
        let json = serde_json::to_value(&response).unwrap();
        assert!(json["configOptions"].is_array());
    }

    #[test]
    fn test_session_config_value_as_id_accepts_value_id_only() {
        let value_id = SessionConfigOptionValue::value_id("ask");
        assert_eq!(session_config_value_as_id(&value_id).as_deref(), Some("ask"));

        let boolean = SessionConfigOptionValue::boolean(true);
        assert!(session_config_value_as_id(&boolean).is_none());
    }

    #[test]
    fn test_tool_call_content_to_raw_output_text_json_and_plain_text() {
        let json_text = loom::tool_source::ToolCallContent::Text("{\"ok\":true}".to_string());
        assert_eq!(
            tool_call_content_to_raw_output(&json_text),
            serde_json::json!({"ok": true})
        );

        let plain_text = loom::tool_source::ToolCallContent::Text("hello".to_string());
        assert_eq!(
            tool_call_content_to_raw_output(&plain_text),
            serde_json::json!({"text": "hello"})
        );
    }

    #[test]
    fn test_tool_call_content_to_raw_output_diff() {
        let diff = loom::tool_source::ToolCallContent::Diff {
            path: "/tmp/a.txt".to_string(),
            old_text: Some("old".to_string()),
            new_text: "new".to_string(),
        };
        assert_eq!(
            tool_call_content_to_raw_output(&diff),
            serde_json::json!({
                "type": "diff",
                "path": "/tmp/a.txt",
                "oldText": "old",
                "newText": "new",
            })
        );
    }
}
