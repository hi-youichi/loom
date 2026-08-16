//! ACP Agent implementation: maps protocol requests to Loom execution.
//!
//! [`LoomAcpAgent`] implements `agent_client_protocol::Agent` and maps ACP requests
//! to Loom sessions and execution. See [`crate::protocol`] for protocol and behavior details.

use crate::agent_registry::AgentRegistry;
use crate::client_capabilities::ClientCapabilitiesInfo;
use crate::content::content_blocks_to_user_content;
use crate::extensions::{register::register_default_extensions, ExtensionRegistry};
use crate::session::{SessionId as OurSessionId, SessionStore};
use crate::session_config_store::SessionConfigStore;
use crate::session_repository::SessionRepository;
use crate::stream_bridge::SessionNotifier;
use crate::tools::{create_acp_tools, ClientBridgeTrait, NoOpClientBridge};
use agent::state::ReActState;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, McpCapabilities, PromptCapabilities, SessionCapabilities,
    SessionCloseCapabilities, SessionDeleteCapabilities, SessionListCapabilities,
    SessionResumeCapabilities,
};
use agent_client_protocol::schema::v1::{
    AuthenticateRequest, AuthenticateResponse, CancelNotification, CloseSessionRequest,
    CloseSessionResponse, DeleteSessionRequest, DeleteSessionResponse, ForkSessionRequest,
    ForkSessionResponse, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
    SessionConfigOptionValue, SessionId, SessionNotification, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse, StopReason,
    Usage,
};
use agent_client_protocol::schema::ProtocolVersion;
use checkpoint::{Checkpointer, JsonSerializer, RunnableConfig};
use checkpoint_sqlite_store::SqliteSaver;
use tool_basic::bash::LocalCommandExecutor;

use agent::run::TypedAnyStreamEvent;
use agent::run::{build_react_config, run_agent_from_config, RunCmd, RunError, RunParams};
use agent::run::{RunCompletion, RunOptions};
use chrono::DateTime;
use config::load_full_config;
use loom_llm::message::UserContent;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

fn canonicalize_existing_directory(
    path: &std::path::Path,
) -> agent_client_protocol::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("cwd must be an existing absolute directory"));
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| {
        agent_client_protocol::Error::invalid_params()
            .data("cwd must be an existing absolute directory")
    })?;
    if !canonical.is_dir() {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("cwd must be an existing absolute directory"));
    }
    Ok(canonical)
}

#[async_trait::async_trait]
pub trait ModelProvider: Send + Sync {
    async fn fetch_models(&self) -> Vec<ModelOption>;
}

struct RealModelProvider;

#[async_trait::async_trait]
impl ModelProvider for RealModelProvider {
    async fn fetch_models(&self) -> Vec<ModelOption> {
        fetch_available_models().await
    }
}

async fn fetch_available_models() -> Vec<ModelOption> {
    let registry = model_spec_core::ModelRegistry::global();

    let providers: Vec<model_spec_core::ProviderConfig> = match load_full_config("loom") {
        Ok(config) => config
            .providers
            .into_iter()
            .map(|p| model_spec_core::ProviderConfig {
                name: p.name,
                base_url: p.base_url,
                api_key: p.api_key,
                provider_type: p.provider_type,
                fetch_models: p.fetch_models.unwrap_or(false),
                cache_ttl: None,
                enable_tier_resolution: true,
                declared_models: p.models.into_iter().map(|m| m.id).collect(),
            })
            .collect(),
        Err(_) => vec![],
    };

    let entries = registry.list_all_models(&providers).await;

    let mut all_models: Vec<ModelOption> = entries
        .into_iter()
        .map(|entry| ModelOption {
            id: entry.id.clone(),
            name: entry.id,
            provider: entry.provider,
        })
        .collect();

    all_models.insert(
        0,
        ModelOption {
            id: "default".to_string(),
            name: "(default)".to_string(),
            provider: String::new(),
        },
    );

    all_models
}

/// Handle for Loom as an ACP Agent. Implements [`Agent`], holds the session store.
/// If [`session_update_tx`](Self::session_update_tx) is set, prompt execution sends
/// session/update notifications through this channel.
pub struct LoomAcpAgent {
    pub(crate) sessions: SessionStore,
    pub(crate) agent_registry: AgentRegistry,
    pub(crate) config_store: SessionConfigStore,
    pub(crate) session_repository: SessionRepository,
    pub(crate) session_update_tx: Option<mpsc::Sender<SessionNotification>>,
    pub(crate) model_provider: Arc<dyn ModelProvider>,
    pub(crate) extension_registry: Arc<ExtensionRegistry>,
}

impl std::fmt::Debug for LoomAcpAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoomAcpAgent")
            .field("sessions", &"..")
            .field("agent_registry", &"..")
            .field("config_store", &"..")
            .field("session_update_tx", &self.session_update_tx.is_some())
            .finish()
    }
}

impl LoomAcpAgent {
    /// Construct a new Agent instance (no session/update sending).
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut extension_registry = ExtensionRegistry::new();
        register_default_extensions(
            &mut extension_registry,
            std::sync::Arc::new(crate::global_events::GlobalEventBus::new()),
        );
        Self::new_with_extension_registry(Arc::new(extension_registry), None)
    }

    pub(crate) fn new_with_extension_registry(
        extension_registry: Arc<ExtensionRegistry>,
        session_update_tx: Option<mpsc::Sender<SessionNotification>>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db_path = checkpoint_sqlite_store::default_memory_db_path();
        let config_store = SessionConfigStore::new(db_path.to_str().unwrap_or_default())
            .map_err(|e| format!("session config store init failed: {e}"))?;
        let session_repository = SessionRepository::new(&db_path)
            .map_err(|e| format!("session repository init failed: {e}"))?;

        let agent = Self {
            sessions: SessionStore::new(),
            agent_registry: AgentRegistry::new(),
            config_store,
            session_repository,
            session_update_tx,
            model_provider: Arc::new(RealModelProvider),
            extension_registry,
        };
        agent.restore_session_metadata()?;
        Ok(agent)
    }

    pub fn with_session_update_tx(
        tx: mpsc::Sender<SessionNotification>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut extension_registry = ExtensionRegistry::new();
        register_default_extensions(
            &mut extension_registry,
            std::sync::Arc::new(crate::global_events::GlobalEventBus::new()),
        );
        Self::new_with_extension_registry(Arc::new(extension_registry), Some(tx))
    }

    pub fn with_model_provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        self.model_provider = provider;
        self
    }

    /// Returns read-only access to the session store.
    #[inline]
    pub fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    fn restore_session_metadata(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for metadata in self.session_repository.list()? {
            let session_id = OurSessionId::new(metadata.session_id);
            self.sessions.create_with_owner(
                session_id.clone(),
                Some(metadata.cwd),
                metadata.thread_id,
                metadata.owner_principal,
            );
            if metadata.lifecycle == "closed" {
                self.sessions.close(&session_id);
            } else {
                // A process-local running turn cannot survive restart.
                self.sessions.reopen(&session_id);
            }
        }
        Ok(())
    }

    /// Fetch available models from all configured providers.
    /// Returns a list of ModelOption for the ACP config_options response.
    /// Uses ModelRegistry for caching and unified model access.
    async fn get_available_models(&self) -> Vec<ModelOption> {
        self.model_provider.fetch_models().await
    }

    /// Returns the model-declared reasoning efforts subset for `model_id`.
    /// When the model is "default"/empty or not found in config, returns `None`
    /// (meaning all effort values are offered).
    async fn get_model_reasoning_efforts(&self, model_id: &str) -> Option<Vec<String>> {
        if model_id.is_empty() || model_id == "default" {
            return None;
        }
        let full_config = load_full_config("loom").ok()?;
        let (_, model_name) = model_spec_core::ModelEntry::parse_id(model_id)?;
        for p in &full_config.providers {
            for m in &p.models {
                if m.id == model_name {
                    return m.reasoning_efforts.clone();
                }
            }
        }
        None
    }

    /// Resolve model configuration with tier awareness.
    /// Priority: ACP explicit model > agent model name > agent tier > default config.
    async fn resolve_model_with_tier_awareness(
        &self,
        session_config: &crate::session::SessionConfig,
    ) -> agent::run::ResolvedModelConfig {
        let start_time = std::time::Instant::now();

        if let Some(ref acp_model) = session_config.model {
            let resolved = agent::run::resolve_model_config(Some(acp_model)).await;
            tracing::info!(
                acp_model = %acp_model,
                agent = %session_config.current_agent,
                resolved_model = %resolved.model.as_deref().unwrap_or("none"),
                resolution_time_ms = start_time.elapsed().as_millis(),
                "Using ACP selected model, overriding agent tier configuration"
            );
            return resolved;
        }

        // Try to get model settings from agent profile
        if let Some(profile) = self
            .agent_registry
            .get_agent_config(&session_config.current_agent)
        {
            if let Some(model_config) = profile.model {
                if let Some(ref model_name) = model_config.name {
                    let resolved = agent::run::resolve_model_config(Some(model_name)).await;
                    tracing::info!(
                        model = %model_name,
                        agent = %session_config.current_agent,
                        resolved_model = %resolved.model.as_deref().unwrap_or("none"),
                        resolution_time_ms = start_time.elapsed().as_millis(),
                        "Using agent configured model name"
                    );
                    return resolved;
                }

                if let Some(tier) = model_config.tier {
                    tracing::debug!(
                        tier = ?tier,
                        agent = %session_config.current_agent,
                        "Starting tier-based model resolution"
                    );

                    let mut config = agent::ReactBuildConfig::from_env();
                    config.model_tier = Some(tier);
                    let resolved_config = agent::resolve_tier_and_build_config(&config).await;

                    if resolved_config.model.is_some() {
                        let resolved = agent::run::ResolvedModelConfig {
                            model: resolved_config.model.clone(),
                            provider: resolved_config.llm_provider.clone(),
                            base_url: resolved_config.openai_base_url.clone(),
                            api_key: resolved_config.openai_api_key.clone(),
                            provider_type: resolved_config.llm_provider.clone(),
                            effort: None,
                            tier: None,
                        };

                        tracing::info!(
                            tier = ?tier,
                            agent = %session_config.current_agent,
                            resolved_model = %resolved.model.as_deref().unwrap_or("none"),
                            resolution_time_ms = start_time.elapsed().as_millis(),
                            "Tier resolution successful"
                        );
                        return resolved;
                    }

                    tracing::warn!(
                        tier = ?tier,
                        agent = %session_config.current_agent,
                        "Tier resolution failed, falling back to default provider config"
                    );
                }
            }
        }

        // Default case: no explicit configuration - provide a safe default model
        tracing::info!(
            agent = %session_config.current_agent,
            "No model or tier configuration, resolving from default provider config"
        );

        if let Ok(full_config) = load_full_config("loom") {
            if let Some(ref pname) = full_config.default_provider {
                if let Some(p) = full_config.providers.iter().find(|p| p.name == *pname) {
                    if let Some(ref model_name) = p.model {
                        let mut resolved = agent::run::resolve_model_config(Some(model_name)).await;
                        if resolved.model.is_some() {
                            if resolved.api_key.is_none() {
                                resolved.api_key = p.api_key.clone();
                            }
                            if resolved.base_url.is_none() {
                                resolved.base_url = p.base_url.clone();
                            }
                            if resolved.provider.is_none() {
                                resolved.provider = Some(p.name.clone());
                            }
                            if resolved.provider_type.is_none() {
                                resolved.provider_type = p.provider_type.clone();
                            }
                            return resolved;
                        }
                    }
                    return agent::run::ResolvedModelConfig {
                        model: p.model.clone(),
                        provider: Some(p.name.clone()),
                        base_url: p.base_url.clone(),
                        api_key: p.api_key.clone(),
                        provider_type: p.provider_type.clone(),
                        effort: None,
                        tier: None,
                    };
                }
            }
        }

        let default_model = config::default_model();
        agent::run::resolve_model_config(Some(&default_model)).await
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
            let notifier = SessionNotifier::new(tx.clone(), session_id.clone());
            notifier.try_send_current_mode(mode_id);
        }

        Ok(())
    }
}

impl Default for LoomAcpAgent {
    fn default() -> Self {
        Self::new().expect("LoomAcpAgent default init failed")
    }
}

impl LoomAcpAgent {
    pub async fn initialize(
        &self,
        args: InitializeRequest,
    ) -> agent_client_protocol::Result<InitializeResponse> {
        tracing::info!(protocol_version = ?args.protocol_version, "initialize called");
        let caps_json = serde_json::to_value(&args.client_capabilities).ok();
        let caps = ClientCapabilitiesInfo::from_json(caps_json);
        tracing::info!(
            terminal = caps.supports_terminal(),
            fs_read = caps.can_read_text_file(),
            fs_write = caps.can_write_text_file(),
            mcp_http = caps.supports_mcp_http(),
            prompt_image = caps.supports_prompt_image(),
            "Client capabilities saved"
        );

        // Build base response with proper schema types (Gap 1, Gap 2, Gap 3 fix).
        // Uses builder API to avoid JSON roundtrip that drops unknown fields.
        //
        // v0.14.0 schema fields actually available:
        //   McpCapabilities       { http, sse, acp(unstable) }    — NO stdio
        //   PromptCapabilities    { image, audio, embedded_context } — NO text, NO resource_link
        //   SessionCapabilities   { list, delete, resume, close }
        //   AgentCapabilities     { load_session, mcp_capabilities, prompt_capabilities,
        //                            session_capabilities, auth }
        let mcp = McpCapabilities::new().http(true).sse(false);
        let prompts = PromptCapabilities::new()
            .image(true)
            .audio(true)
            .embedded_context(true);
        // Only advertise methods registered in `stdio_loop`. Resume/close/delete
        // are added to this response in the same change that registers their
        // handlers.
        let session = SessionCapabilities::new()
            .list(SessionListCapabilities::new())
            .delete(SessionDeleteCapabilities::new())
            .resume(SessionResumeCapabilities::new())
            .close(SessionCloseCapabilities::new());

        let mut extension_meta = serde_json::Map::new();
        extension_meta.insert(
            "loomdesk.dev".to_string(),
            self.extension_registry.build_capability_snapshot(),
        );
        let agent_caps = AgentCapabilities::new()
            .load_session(true)
            .mcp_capabilities(mcp)
            .prompt_capabilities(prompts)
            .session_capabilities(session)
            .meta(extension_meta);

        let protocol_version = ProtocolVersion::V1;
        let response = InitializeResponse::new(protocol_version)
            .agent_info(agent_client_protocol::schema::v1::Implementation::new(
                "loom",
                env!("CARGO_PKG_VERSION"),
            ))
            .agent_capabilities(agent_caps);

        tracing::info!("initialize completed");
        Ok(response)
    }

    pub async fn authenticate(
        &self,
        _args: AuthenticateRequest,
    ) -> agent_client_protocol::Result<AuthenticateResponse> {
        tracing::debug!("authenticate called");
        Ok(AuthenticateResponse::default())
    }

    pub async fn new_session(
        &self,
        args: NewSessionRequest,
    ) -> agent_client_protocol::Result<NewSessionResponse> {
        self.new_session_for_owner(args, "local-anonymous").await
    }

    pub async fn new_session_for_owner(
        &self,
        args: NewSessionRequest,
        owner_principal: &str,
    ) -> agent_client_protocol::Result<NewSessionResponse> {
        tracing::debug!(cwd = ?args.cwd, "new_session called");
        let canonical_cwd = canonicalize_existing_directory(&args.cwd)?;
        // Logging is initialized at startup; this is a no-op if already initialized
        crate::logging::init_logging(Some(&canonical_cwd));

        let working_directory = Some(canonical_cwd);
        let our_id = self
            .sessions
            .create_owned(working_directory, owner_principal);
        let session_id = SessionId::new(our_id.as_str().to_string());
        let entry = self.sessions.get(&our_id).ok_or_else(|| {
            agent_client_protocol::Error::internal_error().data("session missing after creation")
        })?;
        let cwd = entry.working_directory.as_ref().ok_or_else(|| {
            agent_client_protocol::Error::internal_error()
                .data("session cwd missing after creation")
        })?;
        if let Err(error) =
            self.session_repository
                .insert(our_id.as_str(), &entry.thread_id, owner_principal, cwd)
        {
            self.sessions.delete(&our_id);
            return Err(agent_client_protocol::Error::internal_error()
                .data(format!("failed to persist session metadata: {error}")));
        }
        tracing::debug!(session_id = %session_id, "session created");

        // Store MCP servers from ACP session/new request
        if !args.mcp_servers.is_empty() {
            let loom_mcp = crate::mcp_convert::acp_mcp_to_loom(&args.mcp_servers);
            tracing::info!(
                session_id = %session_id,
                acp_count = args.mcp_servers.len(),
                loom_count = loom_mcp.len(),
                "MCP servers from session/new"
            );
            self.sessions.update_mcp_servers(&our_id, loom_mcp);
        }

        let default_mode = self.agent_registry.default_mode_id();
        let current_model = None.or_else(crate::last_model::load).unwrap_or_default();
        let is_default = current_model.is_empty() || current_model == "default";
        self.sessions.update_session_config(&our_id, |c| {
            c.current_agent = default_mode.to_string();
            if !is_default {
                c.model = Some(current_model.clone());
            }
        });
        if !is_default {
            if let Err(e) = self.config_store.set(&our_id, "model", &current_model) {
                tracing::warn!(session_id = %our_id, error = %e, "Failed to persist initial model config");
            }
        }
        let display_model = if is_default {
            "default"
        } else {
            &current_model
        };
        let model_options = self.get_available_models().await;
        let current_mode = default_mode;
        let modes = self.agent_registry.to_session_modes();
        let current_effort = "auto";
        let model_reasoning_efforts = self.get_model_reasoning_efforts(display_model).await;
        let config_options = build_session_config_options(
            current_mode,
            display_model,
            current_effort,
            &modes,
            &model_options,
            model_reasoning_efforts.as_deref(),
        )
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
        // Curator opportunistic hook (Hermes `agent/curator.py` #12 —
        // `maybe_run_curator` is defined at
        // `experimental/curator/src/workflow.rs:290` but had zero callers
        // in `apps/`). On every `session/new`, read the session idle
        // duration via `SkillUsageStore` (cheap mtime-based) and, if
        // the curator's interval gating decides a run is needed,
        // spawn the LLM pass as a detached task so the ACP handshake
        // isn't blocked on the LLM round-trip.
        //
        // Tracked using `tracing::info!` so missing-runs show up in
        // `loom-acp` logs; `maybe_run_curator` itself swallows errors
        // and returns `None`.
        let cwd_for_curator = args.cwd.clone();
        tokio::spawn(async move {
            let skills_path = loom_curator::skill_registry::default_path();
            let cfg = loom_curator::CuratorConfig::default();
            let base_config = agent::ReactBuildConfig::from_env();
            let _ = cwd_for_curator; // reserved for future path resolution
                                     // Priority #17 (Hermes `agent/curator.py` #13): source the
                                     // idle gate from session-state so the auto-spawned path
                                     // honors the same idle threshold as a manual `curator run`.
                                     // Round-2 left this as `default()` with no `idle_for_seconds`
                                     // threading — the curator ran regardless of recent
                                     // activity. We resolve the idle window from the
                                     // `LOOM_CURATOR_IDLE_SECS` env var (default 300s) so
                                     // operators can tune it without a recompile.
            let idle_for_seconds = std::env::var("LOOM_CURATOR_IDLE_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(300);
            tracing::info!(
                "session/new — opportunistically running curator (idle_for_seconds={})",
                idle_for_seconds
            );
            loom_curator::workflow::maybe_run_curator(
                &skills_path,
                &cfg,
                base_config,
                Some(idle_for_seconds),
            )
            .await;
        });
        Ok(NewSessionResponse::new(session_id)
            .modes(self.agent_registry.to_session_mode_state(current_mode))
            .config_options(config_options))
    }

    pub async fn cancel(&self, args: CancelNotification) -> agent_client_protocol::Result<()> {
        tracing::debug!(session_id = %args.session_id, "cancel called");
        let key = OurSessionId::new(args.session_id.to_string());
        self.sessions.cancel_current_generation(&key);
        Ok(())
    }

    pub fn cancel_all(&self) {
        self.sessions.cancel_all_generations();
    }

    pub async fn set_session_config_option(
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
                if value_str == "default" {
                    self.sessions
                        .update_session_config(&key, |c| c.model = None);
                } else {
                    self.sessions
                        .update_session_config(&key, |c| c.model = Some(value_str.clone()));
                }
                crate::last_model::save(&value_str);
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
            "effort" => {
                let valid = matches!(
                    value_str.as_str(),
                    "auto" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh"
                );
                if !valid {
                    return Err(agent_client_protocol::Error::new(
                        -32602,
                        format!("invalid effort value: {}", value_str),
                    ));
                }
                if value_str == "auto" {
                    self.sessions
                        .update_session_config(&key, |c| c.effort = None);
                } else {
                    self.sessions
                        .update_session_config(&key, |c| c.effort = Some(value_str.clone()));
                }
                if let Err(e) = self.config_store.set(&key, "effort", &value_str) {
                    tracing::warn!(session_id = %args.session_id, error = %e, "Failed to persist effort config");
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
        let current_model = entry
            .session_config
            .model
            .clone()
            .unwrap_or_else(|| crate::last_model::load().unwrap_or_default());
        let modes = self.agent_registry.to_session_modes();
        let model_options = self.get_available_models().await;
        let current_effort = entry
            .session_config
            .effort
            .clone()
            .unwrap_or_else(|| "auto".to_string());
        let model_reasoning_efforts = self.get_model_reasoning_efforts(&current_model).await;
        build_set_session_config_option_response(
            &current_mode,
            &current_model,
            &current_effort,
            &modes,
            &model_options,
            model_reasoning_efforts.as_deref(),
        )
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))
    }

    pub async fn set_session_mode(
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

    pub async fn fork_session(
        &self,
        args: ForkSessionRequest,
    ) -> agent_client_protocol::Result<ForkSessionResponse> {
        tracing::debug!(session_id = %args.session_id, cwd = ?args.cwd, "fork_session called");
        let canonical_cwd = canonicalize_existing_directory(&args.cwd)?;
        // Logging is initialized at startup; this is a no-op if already initialized
        crate::logging::init_logging(Some(&canonical_cwd));

        let source_key = OurSessionId::new(args.session_id.to_string());
        let source_entry = self
            .sessions
            .get(&source_key)
            .ok_or_else(|| agent_client_protocol::Error::new(-32602, "unknown session"))?;
        if source_entry.working_directory.as_ref() != Some(&canonical_cwd) {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("cwd does not match the session working directory"));
        }

        // Create new session with the same working directory and config
        let new_our_id = self.sessions.create_owned(
            source_entry.working_directory.clone(),
            source_entry.owner_principal.clone(),
        );
        let new_session_id = SessionId::new(new_our_id.as_str().to_string());
        let new_entry = self.sessions.get(&new_our_id).ok_or_else(|| {
            agent_client_protocol::Error::internal_error()
                .data("forked session missing after creation")
        })?;
        self.session_repository
            .insert(
                new_our_id.as_str(),
                &new_entry.thread_id,
                &new_entry.owner_principal,
                new_entry.working_directory.as_ref().ok_or_else(|| {
                    agent_client_protocol::Error::internal_error()
                        .data("forked session cwd missing")
                })?,
            )
            .map_err(|error| {
                self.sessions.delete(&new_our_id);
                agent_client_protocol::Error::internal_error()
                    .data(format!("failed to persist forked session: {error}"))
            })?;

        // Copy source session config (model, mode) to the new session
        self.sessions.update_session_config(&new_our_id, |c| {
            *c = source_entry.session_config.clone();
        });

        // Inherit MCP servers from source session
        self.sessions
            .update_mcp_servers(&new_our_id, source_entry.mcp_servers.clone());

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
        let current_model = source_entry
            .session_config
            .model
            .clone()
            .unwrap_or_else(|| {
                std::env::var("MODEL")
                    .ok()
                    .or_else(crate::last_model::load)
                    .unwrap_or_default()
            });
        // If model was resolved from fallback rather than source config, persist it
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
        let current_effort = source_entry
            .session_config
            .effort
            .clone()
            .unwrap_or_else(|| "auto".to_string());
        let model_reasoning_efforts = self.get_model_reasoning_efforts(&current_model).await;
        let config_options = build_session_config_options(
            &current_mode,
            &current_model,
            &current_effort,
            &modes,
            &model_options,
            model_reasoning_efforts.as_deref(),
        )
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;

        Ok(ForkSessionResponse::new(new_session_id)
            .modes(self.agent_registry.to_session_mode_state(&current_mode))
            .config_options(config_options))
    }

    pub async fn prompt(
        &self,
        args: PromptRequest,
    ) -> agent_client_protocol::Result<PromptResponse> {
        self.prompt_with_capabilities(
            args,
            ClientCapabilitiesInfo::default(),
            Arc::new(NoOpClientBridge),
        )
        .await
    }

    /// Execute a prompt using capabilities from the caller's transport.
    pub async fn prompt_with_capabilities(
        &self,
        args: PromptRequest,
        client_capabilities: ClientCapabilitiesInfo,
        client_bridge: Arc<dyn ClientBridgeTrait>,
    ) -> agent_client_protocol::Result<PromptResponse> {
        tracing::debug!(session_id = %args.session_id, prompt_blocks = args.prompt.len(), "prompt called");
        let key = OurSessionId::new(args.session_id.to_string());
        let entry = self
            .sessions
            .get(&key)
            .ok_or_else(|| agent_client_protocol::Error::new(-32602, "unknown session"))?;

        let cancellation = self.sessions.begin_prompt(&key).ok_or_else(|| {
            agent_client_protocol::Error::new(
                -32010,
                "a prompt is already in progress for this session",
            )
        })?;

        // RAII guard: ensures finish_prompt is called even if the future is
        // dropped (e.g., WS disconnect cancels the task mid-prompt).
        let _prompt_guard =
            crate::session::PromptGuard::new(&self.sessions, &key, cancellation.generation());

        let user_content =
            content_blocks_to_user_content(args.prompt.as_slice()).map_err(|_| {
                agent_client_protocol::Error::new(-32602, "content_blocks parse failed")
            })?;

        if let loom_llm::message::UserContent::Text(ref text) = user_content {
            if let Some(cmd) = agent::commands::parse(text) {
                match cmd {
                    agent::commands::Command::ResetContext => {
                        self.sessions.cancel_current_generation(&key);
                        tracing::info!(session_id = %args.session_id, "Context cleared via /reset command");
                        return Ok(PromptResponse::new(StopReason::EndTurn));
                    }
                    agent::commands::Command::Goal { description } => {
                        tracing::info!(
                            session_id = %args.session_id,
                            goal = %description,
                            "Goal mode activated via /goal command"
                        );
                        let working_folder = entry.working_directory.clone().ok_or_else(|| {
                            agent_client_protocol::Error::internal_error()
                                .data("ACP session has no working directory")
                        })?;

                        let resolved_goal = self
                            .resolve_model_with_tier_awareness(&entry.session_config)
                            .await;
                        let goal_ctx_window =
                            resolve_context_window_size(resolved_goal.model.as_deref()).await;

                        let event_sender: Option<
                            std::sync::Arc<dyn Fn(agent::run::TypedAnyStreamEvent) + Send + Sync>,
                        > = self.session_update_tx.clone().map(|sender| {
                            let session_id = args.session_id.clone();
                            std::sync::Arc::new(move |ev: agent::run::TypedAnyStreamEvent| {
                                let notifier =
                                    SessionNotifier::new(sender.clone(), session_id.clone())
                                        .with_context_window_size(goal_ctx_window);
                                notifier.try_send_stream_event(&ev);
                            })
                                as std::sync::Arc<
                                    dyn Fn(agent::run::TypedAnyStreamEvent) + Send + Sync,
                                >
                        });

                        let cancel = tokio_util::sync::CancellationToken::new();

                        let result = crate::goal_runner::run_goal(
                            description,
                            working_folder,
                            cancel,
                            event_sender,
                            Some(cancellation.clone()),
                        )
                        .await;

                        match result {
                            Ok(goal_result) => {
                                tracing::info!(
                                    session_id = %args.session_id,
                                    task_id = %goal_result.task_id,
                                    outcome = %goal_result.outcome,
                                    "Goal finished"
                                );
                                return Ok(PromptResponse::new(StopReason::EndTurn));
                            }
                            Err(e) => {
                                tracing::error!(
                                    session_id = %args.session_id,
                                    error = %e,
                                    "Goal run failed"
                                );
                                return Ok(PromptResponse::new(StopReason::EndTurn));
                            }
                        }
                    }
                    agent::commands::Command::ReviewSkill { scope } => {
                        tracing::info!(
                            session_id = %args.session_id,
                            scope = ?scope,
                            "Review-skill command triggered"
                        );
                        let resolved = self
                            .resolve_model_with_tier_awareness(&entry.session_config)
                            .await;
                        let (review_memory, review_skills) =
                            crate::review_runner::scope_to_review_config(&scope);
                        crate::review_runner::spawn_inprocess_review(
                            entry.thread_id.clone(),
                            resolved,
                            review_memory,
                            review_skills,
                            "review-skill".to_string(),
                            self.session_update_tx.clone(),
                            Some(args.session_id.clone()),
                        );
                        return Ok(PromptResponse::new(StopReason::EndTurn));
                    }
                    agent::commands::Command::Models { .. }
                    | agent::commands::Command::ModelsUse { .. } => {
                        // ACP handles models via SetSessionConfigOption, not here
                    }
                    _ => {
                        return Ok(PromptResponse::new(StopReason::EndTurn));
                    }
                }
            }
        }

        let content_type = match &user_content {
            loom_llm::message::UserContent::Text(_) => "text",
            loom_llm::message::UserContent::Multimodal(parts) => {
                let has_image = parts
                    .iter()
                    .any(|p| matches!(p, loom_llm::message::ContentPart::ImageBase64 { .. }));
                let has_audio = parts
                    .iter()
                    .any(|p| matches!(p, loom_llm::message::ContentPart::AudioBase64 { .. }));
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

        let working_folder = entry.working_directory.clone().ok_or_else(|| {
            agent_client_protocol::Error::internal_error()
                .data("ACP session has no working directory")
        })?;

        let resolved = self
            .resolve_model_with_tier_awareness(&entry.session_config)
            .await;
        let resolved_for_review = resolved.clone();

        let context_window_size = resolve_context_window_size(resolved.model.as_deref()).await;

        let opts = RunOptions {
            message: user_content,
            working_folder: Some(working_folder),
            session_id: None,
            cancellation: Some(cancellation.clone()),
            thread_id: Some(entry.thread_id.clone()),
            agent: Some(
                self.agent_registry
                    .resolve_agent_name(&entry.session_config.current_agent),
            ),
            verbose: false,
            verbose_level: 0,
            got_adaptive: false,
            display_max_len: 4096,
            output_json: false,
            model: resolved.model,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
            debug_llm: false,
            provider: resolved.provider,
            base_url: resolved.base_url,
            api_key: resolved.api_key,
            provider_type: resolved.provider_type,
            any_stream_event_sender: None,
            acp_session_id: Some(args.session_id.to_string()),
            bash_executor: {
                if client_capabilities.supports_terminal() {
                    tracing::info!("Using ACP client terminal executor");
                    Some(Arc::new(crate::tools::AcpBridgeCommandExecutor::new(
                        client_bridge.clone(),
                    ))
                        as Arc<dyn tool_basic::bash::CommandExecutor>)
                } else {
                    tracing::info!("Using local bash executor (ACP terminal unavailable)");
                    Some(Arc::new(LocalCommandExecutor)
                        as Arc<dyn tool_basic::bash::CommandExecutor>)
                }
            },
            extra_tools: {
                let tools = create_acp_tools(&client_capabilities, client_bridge.clone());
                if tools.is_empty() {
                    None
                } else {
                    tracing::info!(count = tools.len(), "Registering ACP tools");
                    Some(Arc::new(
                        tools
                            .into_iter()
                            .map(|t| Arc::from(t) as Arc<dyn tool_core::Tool>)
                            .collect(),
                    ))
                }
            },
            default_extra_tools_provider: Some(tool_workflow::default_workflow_tool_provider()),
            force_compact: false,
            chat_id: None,
            worktree: false,
            goal_mode: false,
            acp_mcp_servers: if entry.mcp_servers.is_empty() {
                None
            } else {
                Some(entry.mcp_servers.clone())
            },
            acp_mcp_sources: if entry.mcp_servers.is_empty() {
                None
            } else {
                Some(
                    entry
                        .mcp_runtime
                        .ensure_sources(&entry.mcp_servers)
                        .await
                        .map_err(|error| {
                            agent_client_protocol::Error::internal_error().data(error)
                        })?,
                )
            },
            effort: resolved.effort,
            tier: resolved.tier,
        };

        let session_id = args.session_id.clone();
        let tx = self.session_update_tx.clone();
        let usage_acc: Arc<Mutex<TurnUsage>> = Arc::new(Mutex::new(TurnUsage::default()));
        let on_event: Option<Box<dyn FnMut(TypedAnyStreamEvent) + Send>> = {
            let acc = usage_acc.clone();
            match tx {
                Some(ref sender) => {
                    let notifier = SessionNotifier::new(sender.clone(), session_id.clone())
                        .with_context_window_size(context_window_size)
                        .with_usage_acc(acc.clone());

                    // Enable high-frequency tracking with estimated base usage
                    // Base usage estimated from prompt message (rough approximation)
                    let estimated_base_tokens = match &opts.message {
                        UserContent::Text(text) => text.len() / 4, // Approx 4 chars per token
                        UserContent::Multimodal(parts) => {
                            parts
                                .iter()
                                .map(|p| {
                                    match p {
                                        loom_llm::message::ContentPart::Text { text } => {
                                            text.len() / 4
                                        }
                                        _ => 0, // Non-text parts estimated as 0 tokens
                                    }
                                })
                                .sum::<usize>()
                        }
                    };
                    notifier.enable_high_freq_tracking(
                        estimated_base_tokens as u64,
                        context_window_size,
                    );

                    let title_repository = self.session_repository.clone();
                    let title_session_id = session_id.to_string();
                    let closure = move |ev: TypedAnyStreamEvent| {
                        capture_turn_usage(&ev, &acc);
                        persist_session_title(&title_repository, &title_session_id, &ev);
                        notifier.try_send_event(&ev);
                    };
                    Some(Box::new(closure) as Box<dyn FnMut(TypedAnyStreamEvent) + Send>)
                }
                None => None,
            }
        };

        let (config, _, _) = build_react_config(&opts);
        let result = run_agent_from_config(
            &config,
            &RunCmd::React,
            RunParams {
                message: opts.message.clone(),
                verbose: opts.verbose,
                cancellation: opts.cancellation.clone(),
                any_stream_event_sender: opts.any_stream_event_sender.clone(),
                llm_override: None,
            },
            on_event,
        )
        .await;

        // Disable high-frequency tracking after agent execution
        if let Some(ref tx) = tx {
            let notifier = SessionNotifier::new(tx.clone(), session_id.clone());
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                notifier.disable_high_freq_tracking().await;
            });
        }

        self.sessions.finish_prompt(&key, cancellation.generation());

        match result {
            Ok(RunCompletion::Finished(_reply)) => {
                crate::review_runner::spawn_inprocess_review(
                    entry.thread_id.clone(),
                    resolved_for_review,
                    true,
                    true,
                    "background".to_string(),
                    self.session_update_tx.clone(),
                    Some(session_id),
                );
                let mut resp = PromptResponse::new(StopReason::EndTurn);
                if let Some(usage) = build_acp_usage(&usage_acc) {
                    resp = resp.usage(usage);
                }
                Ok(resp)
            }
            Ok(RunCompletion::Cancelled) => Ok(PromptResponse::new(StopReason::Cancelled)),
            Err(e) => {
                tracing::error!(session_id = %args.session_id, error = %e, "run_agent errored");
                Err(map_run_error(e))
            }
        }
    }

    pub async fn load_session(
        &self,
        args: LoadSessionRequest,
    ) -> agent_client_protocol::Result<LoadSessionResponse> {
        self.load_session_for_owner(args, "local-anonymous").await
    }

    pub async fn load_session_for_owner(
        &self,
        args: LoadSessionRequest,
        owner_principal: &str,
    ) -> agent_client_protocol::Result<LoadSessionResponse> {
        tracing::info!(session_id = %args.session_id, cwd = ?args.cwd, "load_session started");
        let canonical_cwd = canonicalize_existing_directory(&args.cwd)?;
        // Logging is initialized at startup; this is a no-op if already initialized
        crate::logging::init_logging(Some(&canonical_cwd));
        let session_id = args.session_id.clone();
        let our_session_id = OurSessionId::new(session_id.to_string());
        let entry = if let Some(existing) = self.sessions.get(&our_session_id) {
            if existing.owner_principal != owner_principal {
                return Err(agent_client_protocol::Error::new(
                    -32000,
                    "session not available for this principal",
                ));
            }
            if existing.working_directory.as_ref() != Some(&canonical_cwd) {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("cwd does not match the session working directory"));
            }
            tracing::info!(
                session_id = %session_id,
                thread_id = %existing.thread_id,
                "Reusing existing session entry from memory"
            );
            existing
        } else {
            let metadata = self
                .session_repository
                .get(our_session_id.as_str())
                .map_err(|error| {
                    agent_client_protocol::Error::internal_error()
                        .data(format!("failed to read session metadata: {error}"))
                })?
                .ok_or_else(|| agent_client_protocol::Error::new(-32002, "session not found"))?;
            if metadata.owner_principal != owner_principal {
                return Err(agent_client_protocol::Error::new(
                    -32000,
                    "session not available for this principal",
                ));
            }
            if metadata.cwd != canonical_cwd {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("cwd does not match the session working directory"));
            }
            tracing::info!(
                session_id = %session_id,
                thread_id = %metadata.thread_id,
                "Restoring session entry from durable metadata"
            );
            self.sessions.create_with_owner(
                our_session_id.clone(),
                Some(metadata.cwd),
                metadata.thread_id,
                metadata.owner_principal,
            );
            let default_mode = self.agent_registry.default_mode_id();
            self.sessions.update_session_config(&our_session_id, |c| {
                if c.current_agent.is_empty() {
                    c.current_agent = default_mode.to_string();
                }
            });
            self.sessions.get(&our_session_id).ok_or_else(|| {
                tracing::error!(session_id = %our_session_id, "Session not found after creation");
                agent_client_protocol::Error::internal_error().data(format!(
                    "Session {} not found after creation",
                    our_session_id
                ))
            })?
        };

        let db_path = checkpoint_sqlite_store::default_memory_db_path();
        tracing::debug!(
            session_id = %session_id,
            thread_id = %entry.thread_id,
            db_path = %db_path.display(),
            "Querying checkpoint for session history"
        );
        let serializer = Arc::new(JsonSerializer);
        let checkpointer: Arc<dyn Checkpointer<ReActState>> = Arc::new(
            SqliteSaver::new(db_path.to_string_lossy().as_ref(), serializer).map_err(|e| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("Failed to create checkpointer: {}", e))
            })?,
        );

        let config = RunnableConfig {
            thread_id: Some(entry.thread_id.clone()),
            checkpoint_id: None,
            checkpoint_ns: String::new(),
            user_id: None,
            resume_from_node_id: None,
            depth: None,
            acp_session_id: None,
            resume_value: None,
            resume_values_by_namespace: Default::default(),
            resume_values_by_interrupt_id: Default::default(),
        };

        match checkpointer.get_tuple(&config).await {
            Ok(Some((checkpoint, _metadata))) => {
                let state: ReActState = checkpoint.channel_values;
                let user_count = state
                    .messages
                    .iter()
                    .filter(|m| matches!(m, loom_llm::message::Message::User(_)))
                    .count();
                let assistant_count = state
                    .messages
                    .iter()
                    .filter(|m| matches!(m, loom_llm::message::Message::Assistant(_)))
                    .count();
                let tool_count = state
                    .messages
                    .iter()
                    .filter(|m| matches!(m, loom_llm::message::Message::Tool { .. }))
                    .count();
                let system_count = state
                    .messages
                    .iter()
                    .filter(|m| matches!(m, loom_llm::message::Message::System(_)))
                    .count();

                tracing::info!(
                    session_id = %session_id,
                    thread_id = %entry.thread_id,
                    total = state.messages.len(),
                    user = user_count,
                    assistant = assistant_count,
                    tool = tool_count,
                    system = system_count,
                    "Checkpoint found, replaying session history"
                );

                if let Some(ref tx) = self.session_update_tx {
                    let notifier = SessionNotifier::new(tx.clone(), session_id.clone());
                    notifier.send_history(&state.messages).await;
                } else {
                    tracing::warn!(
                        session_id = %session_id,
                        "No session_update_tx available, history not sent to client"
                    );
                }

                tracing::info!(
                    session_id = %session_id,
                    message_count = state.messages.len(),
                    "Session history replay completed"
                );
            }
            Ok(None) => {
                tracing::info!(
                    session_id = %session_id,
                    thread_id = %entry.thread_id,
                    "No checkpoint found for session, starting fresh"
                );
            }
            Err(e) => {
                tracing::warn!(
                    session_id = %session_id,
                    thread_id = %entry.thread_id,
                    error = %e,
                    "Failed to load checkpoint, starting fresh"
                );
            }
        }

        // Convert and store MCP servers from the load request
        if !args.mcp_servers.is_empty() {
            let loom_mcp = crate::mcp_convert::acp_mcp_to_loom(&args.mcp_servers);
            tracing::info!(
                session_id = %session_id,
                acp_count = args.mcp_servers.len(),
                loom_count = loom_mcp.len(),
                "MCP servers from session/load"
            );
            self.sessions.update_mcp_servers(&our_session_id, loom_mcp);
        }

        // Return LoadSessionResponse with config_options and modes
        // First, try to load from persistent store
        let persisted_config = self.config_store.get_all(&our_session_id)
            .unwrap_or_else(|e| {
                tracing::warn!(session_id = %session_id, error = %e, "Failed to load persistent config");
                std::collections::HashMap::new()
            });

        let current_mode = persisted_config.get("mode").cloned().unwrap_or_else(|| {
            if entry.session_config.current_agent.is_empty() {
                self.agent_registry.default_mode_id().to_string()
            } else {
                entry.session_config.current_agent.clone()
            }
        });

        let current_model = persisted_config.get("model").cloned().unwrap_or_else(|| {
            entry
                .session_config
                .model
                .clone()
                .unwrap_or_else(|| crate::last_model::load().unwrap_or_default())
        });

        let current_effort = persisted_config.get("effort").cloned().unwrap_or_else(|| {
            entry
                .session_config
                .effort
                .clone()
                .unwrap_or_else(|| "auto".to_string())
        });
        if current_effort == "auto" {
            self.sessions
                .update_session_config(&our_session_id, |c| c.effort = None);
        } else {
            self.sessions.update_session_config(&our_session_id, |c| {
                c.effort = Some(current_effort.clone())
            });
        }

        let model_options = self.get_available_models().await;
        let available_modes = self.agent_registry.to_session_modes();
        let model_reasoning_efforts = self.get_model_reasoning_efforts(&current_model).await;
        let config_options = build_session_config_options(
            &current_mode,
            &current_model,
            &current_effort,
            &available_modes,
            &model_options,
            model_reasoning_efforts.as_deref(),
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
        self.sessions.reopen(&our_session_id);
        self.session_repository
            .set_lifecycle(our_session_id.as_str(), "idle")
            .map_err(|error| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("failed to persist session lifecycle: {error}"))
            })?;
        Ok(response)
    }

    pub async fn resume_session_for_owner(
        &self,
        args: ResumeSessionRequest,
        owner_principal: &str,
    ) -> agent_client_protocol::Result<ResumeSessionResponse> {
        let canonical_cwd = canonicalize_existing_directory(&args.cwd)?;
        let session_id = args.session_id.clone();
        let key = OurSessionId::new(session_id.to_string());
        let entry = self
            .sessions
            .get(&key)
            .ok_or_else(|| agent_client_protocol::Error::new(-32002, "session not found"))?;
        if entry.owner_principal != owner_principal {
            return Err(agent_client_protocol::Error::new(
                -32000,
                "session not available for this principal",
            ));
        }
        if entry.working_directory.as_ref() != Some(&canonical_cwd) {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("cwd does not match the session working directory"));
        }
        if !args.mcp_servers.is_empty() {
            self.sessions
                .update_mcp_servers(&key, crate::mcp_convert::acp_mcp_to_loom(&args.mcp_servers));
        }
        self.sessions.reopen(&key);
        self.session_repository
            .set_lifecycle(key.as_str(), "idle")
            .map_err(|error| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("failed to persist session lifecycle: {error}"))
            })?;
        let mode = if entry.session_config.current_agent.is_empty() {
            self.agent_registry.default_mode_id().to_string()
        } else {
            entry.session_config.current_agent
        };
        Ok(ResumeSessionResponse::new().modes(self.agent_registry.to_session_mode_state(&mode)))
    }

    pub async fn close_session_for_owner(
        &self,
        args: CloseSessionRequest,
        owner_principal: &str,
    ) -> agent_client_protocol::Result<CloseSessionResponse> {
        let key = OurSessionId::new(args.session_id.to_string());
        let Some(entry) = self.sessions.get(&key) else {
            return Ok(CloseSessionResponse::new());
        };
        if entry.owner_principal != owner_principal {
            return Err(agent_client_protocol::Error::new(
                -32000,
                "session not available for this principal",
            ));
        }
        self.sessions.close(&key);
        self.session_repository
            .set_lifecycle(key.as_str(), "closed")
            .map_err(|error| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("failed to persist session lifecycle: {error}"))
            })?;
        Ok(CloseSessionResponse::new())
    }

    pub async fn delete_session_for_owner(
        &self,
        args: DeleteSessionRequest,
        owner_principal: &str,
    ) -> agent_client_protocol::Result<DeleteSessionResponse> {
        let key = OurSessionId::new(args.session_id.to_string());
        let Some(entry) = self.sessions.get(&key) else {
            return Ok(DeleteSessionResponse::new());
        };
        if entry.owner_principal != owner_principal {
            return Err(agent_client_protocol::Error::new(
                -32000,
                "session not available for this principal",
            ));
        }
        self.session_repository
            .delete_all(key.as_str(), &entry.thread_id)
            .map_err(|error| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("failed to delete session data: {error}"))
            })?;
        self.sessions.delete(&key);
        Ok(DeleteSessionResponse::new())
    }

    pub async fn list_sessions(
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
        let protocol_sessions: Vec<agent_client_protocol::schema::v1::SessionInfo> = our_sessions
            .into_iter()
            .map(|s| {
                // Convert cwd: Option<String> to PathBuf string (use default if None)
                let cwd_str = s
                    .cwd
                    .unwrap_or_else(|| agent::run::DEFAULT_WORKING_FOLDER.to_string());
                let cwd_path = std::path::PathBuf::from(&cwd_str);

                let mut session_json = serde_json::json!({
                    "sessionId": s.session_id,
                    "cwd": cwd_path,
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
                    if let Some(review) = meta.review {
                        if let Ok(review_val) = serde_json::to_value(&review) {
                            meta_map.insert("review".to_string(), review_val);
                        }
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

    /// List only durable sessions owned by the authenticated ACP principal.
    /// Metadata is the source of truth so a newly-created session is visible
    /// even before its first checkpoint is written.
    pub async fn list_sessions_for_owner(
        &self,
        args: ListSessionsRequest,
        owner_principal: &str,
    ) -> agent_client_protocol::Result<ListSessionsResponse> {
        let cwd_filter = match args.cwd {
            Some(cwd) => Some(canonicalize_existing_directory(&cwd)?),
            None => None,
        };
        let sessions = self
            .session_repository
            .list_for_owner(owner_principal)
            .map_err(|error| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("failed to list session metadata: {error}"))
            })?
            .into_iter()
            .filter(|metadata| {
                cwd_filter
                    .as_ref()
                    .map(|cwd| &metadata.cwd == cwd)
                    .unwrap_or(true)
            })
            .map(|metadata| {
                let mut info = agent_client_protocol::schema::v1::SessionInfo::new(
                    metadata.session_id,
                    metadata.cwd,
                );
                info.title = metadata.title;
                info.updated_at = metadata.updated_at;
                info
            })
            .collect();
        Ok(ListSessionsResponse::new(sessions))
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
    /// Background review status for this session (latest record).
    /// `None` when the session has never been reviewed (implicitly "pending").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<SessionReviewMeta>,
}

/// Background review status for a session, derived from the latest
/// `review_status` row (updated on every prompt completion and on
/// `/review-skill`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionReviewMeta {
    /// "reviewed" | "skipped"
    pub status: String,
    /// ISO 8601 timestamp of the most recent review
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<String>,
    /// Number of memory updates produced by the review
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_update_count: Option<usize>,
    /// Number of skill updates produced by the review
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_update_count: Option<usize>,
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
    ) -> Result<Vec<crate::agent::SessionInfo>, agent_client_protocol::Error> {
        let db_path = checkpoint_sqlite_store::default_memory_db_path();
        let cwd_filter = cwd_filter.map(String::from);

        // Use spawn_blocking for SQLite operations
        let sessions = tokio::task::spawn_blocking(move || -> Result<Vec<SessionInfo>, String> {
            let conn = Connection::open(&db_path)
                .map_err(|e| format!("Failed to open database: {}", e))?;

            // Ensure the review_status table exists before we LEFT JOIN against
            // it. On a fresh database with no review ever recorded the table
            // would otherwise be absent, causing the query to fail.
            loom_curator::ReviewHistory::ensure_schema(&conn)
                .map_err(|e| format!("Failed to ensure review schema: {}", e))?;

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
                    lc.metadata_summary as latest_summary,
                    rs.status as review_status,
                    rs.reviewed_at as review_reviewed_at,
                    rs.memory_update_count as review_memory_count,
                    rs.skill_update_count as review_skill_count
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
                LEFT JOIN review_status rs ON rs.session_id = c.thread_id
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
                    let review_status_str: Option<String> = row.get(7)?;
                    let review_reviewed_at_ms: Option<i64> = row.get(8)?;
                    let review_memory_count: Option<i64> = row.get(9)?;
                    let review_skill_count: Option<i64> = row.get(10)?;

                    let updated_at = last_updated_ms
                        .and_then(DateTime::from_timestamp_millis)
                        .map(|dt| dt.to_rfc3339());

                    // Use summary as title if available and not empty, otherwise generate from session_id
                    let title = latest_summary.filter(|s| !s.trim().is_empty()).or_else(|| {
                        Some(format!(
                            "Session {}",
                            session_id.chars().take(8).collect::<String>()
                        ))
                    });

                    // LEFT JOIN yields NULL for all rs.* columns when the
                    // session has never been reviewed; in that case `review`
                    // stays None (implicitly "pending").
                    let review = review_status_str.map(|status| SessionReviewMeta {
                        status,
                        reviewed_at: review_reviewed_at_ms
                            .and_then(DateTime::from_timestamp_millis)
                            .map(|dt| dt.to_rfc3339()),
                        memory_update_count: review_memory_count.map(|i| i as usize),
                        skill_update_count: review_skill_count.map(|i| i as usize),
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
                            review,
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

#[derive(Default, Clone, Debug)]
pub(crate) struct TurnUsage {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) total_tokens: u64,
    pub(crate) cached_tokens: u64,
}

/// Persist the fire-and-forget first-turn title event
/// (`Updates { node_id: "title" }`) into the session repository so
/// `session/list` can serve the title across restarts. The push to the
/// client happens separately via `SessionNotifier` (`session_info_update`).
fn persist_session_title(
    repository: &SessionRepository,
    session_id: &str,
    ev: &TypedAnyStreamEvent,
) {
    if let TypedAnyStreamEvent::React(stream_event::StreamEvent::Updates { node_id, state, .. }) =
        ev
    {
        if node_id == "title" {
            if let Some(title) = state.summary.as_ref().filter(|t| !t.trim().is_empty()) {
                if let Err(error) = repository.set_title(session_id, title) {
                    tracing::warn!(session_id, error = %error, "failed to persist session title");
                }
            }
        }
    }
}

fn extract_llm_usage<S>(ev: &stream_event::StreamEvent<S>) -> Option<(u32, u32, u32, Option<u32>)>
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    match ev {
        stream_event::StreamEvent::TurnFinish { usage, .. } => Some((
            usage.input,
            usage.output,
            usage.input + usage.output,
            usage.cache_read,
        )),
        _ => None,
    }
}

fn extract_llm_usage_from_any(ev: &TypedAnyStreamEvent) -> Option<(u32, u32, u32, Option<u32>)> {
    match ev {
        TypedAnyStreamEvent::React(e) => extract_llm_usage(e),
        TypedAnyStreamEvent::Dup(e) => extract_llm_usage(e),
        TypedAnyStreamEvent::Tot(e) => extract_llm_usage(e),
        TypedAnyStreamEvent::Got(e) => extract_llm_usage(e),
    }
}

pub(crate) fn capture_turn_usage(ev: &TypedAnyStreamEvent, acc: &Mutex<TurnUsage>) {
    if let Some((prompt, completion, total, cached)) = extract_llm_usage_from_any(ev) {
        let mut a = acc.lock().unwrap();
        a.input_tokens += prompt as u64;
        a.output_tokens += completion as u64;
        a.total_tokens += total as u64;
        if let Some(c) = cached {
            a.cached_tokens += c as u64;
        }
    }
}

fn build_acp_usage(acc: &Mutex<TurnUsage>) -> Option<Usage> {
    let a = acc.lock().unwrap();
    if a.total_tokens == 0 {
        return None;
    }
    let mut usage = Usage::new(a.total_tokens, a.input_tokens, a.output_tokens);
    if a.cached_tokens > 0 {
        usage = usage.cached_read_tokens(a.cached_tokens);
    }
    Some(usage)
}

/// Model option for ACP config dropdown.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelOption {
    pub id: String,
    pub name: String,
    pub provider: String,
}

/// If `current_model` is a bare model id (e.g. from `MODEL=`) but options use `provider/model`,
/// rewrite to the single matching option when unambiguous.
fn normalize_current_model_for_acp(current_model: &str, options: &[ModelOption]) -> String {
    if current_model.is_empty() || current_model == "default" {
        return "default".to_string();
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
/// SessionConfigOption has kind flattened; SessionConfigKind uses tag "type" ? "type": "select" and SessionConfigSelect fields at top level (camelCase).
fn build_session_config_options(
    current_mode: &str,
    current_model: &str,
    current_effort: &str,
    modes: &[agent_client_protocol::schema::v1::SessionMode],
    model_options: &[ModelOption],
    model_reasoning_efforts: Option<&[String]>,
) -> Result<Vec<agent_client_protocol::schema::v1::SessionConfigOption>, serde_json::Error> {
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

    let all_efforts = [
        ("auto", "Auto", "Use model default"),
        ("none", "None", "No reasoning"),
        ("minimal", "Minimal", "Minimal reasoning"),
        ("low", "Low", "Low effort"),
        ("medium", "Medium", "Balanced"),
        ("high", "High", "Deep reasoning"),
        ("xhigh", "Xhigh", "Deepest reasoning"),
    ];
    let effort_options: Vec<_> = all_efforts
        .iter()
        .filter(|(v, _, _)| {
            *v == "auto" || model_reasoning_efforts.is_none_or(|r| r.iter().any(|e| e == v))
        })
        .map(|(v, n, d)| serde_json::json!({"value": v, "name": n, "description": d}))
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
        },
        {
            "id": "effort",
            "name": "Reasoning Effort",
            "description": "Controls reasoning depth for thinking models.",
            "category": "thought_level",
            "type": "select",
            "currentValue": current_effort,
            "options": effort_options
        }
    ]);
    serde_json::from_value(json)
}

/// Build SetSessionConfigOptionResponse with a single "model" option (protocol types are non_exhaustive, so we construct via serde).
fn build_set_session_config_option_response(
    current_mode: &str,
    current_model: &str,
    current_effort: &str,
    modes: &[agent_client_protocol::schema::v1::SessionMode],
    model_options: &[ModelOption],
    model_reasoning_efforts: Option<&[String]>,
) -> Result<SetSessionConfigOptionResponse, serde_json::Error> {
    let config_options = build_session_config_options(
        current_mode,
        current_model,
        current_effort,
        modes,
        model_options,
        model_reasoning_efforts,
    )?;
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

/// Resolve model context window from model spec, falling back to 128k default.
async fn resolve_context_window_size(model: Option<&str>) -> u64 {
    use model_spec_core::resolver::{ConfigModelEntry, ConfigProviderEntry};

    let Some(model) = model else {
        return agent::compress::CompactionConfig::default().max_context_tokens as u64;
    };

    let providers: Vec<ConfigProviderEntry> = config::load_full_config("loom")
        .ok()
        .map(|f| {
            f.providers
                .into_iter()
                .filter(|p| !p.models.is_empty())
                .map(|p| ConfigProviderEntry {
                    name: p.name,
                    models: p
                        .models
                        .into_iter()
                        .map(|m| ConfigModelEntry {
                            id: m.id,
                            context_limit: m.context_limit,
                            output_limit: m.output_limit,
                        })
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    match model_spec_core::resolver::resolve_model_context_limit(model, providers).await {
        Some(context_limit) => {
            tracing::info!(
                model = %model,
                context_limit,
                "resolved context window size from model spec"
            );
            context_limit as u64
        }
        None => {
            tracing::warn!(
                model = %model,
                "model spec not found, using default 128k context window"
            );
            agent::compress::CompactionConfig::default().max_context_tokens as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_config_select_option_structure() {
        use agent_client_protocol::schema::v1::{SessionConfigSelectOption, SessionConfigValueId};

        let option_id = SessionConfigValueId::new("gpt-4o".to_string());
        let select_option = SessionConfigSelectOption::new(option_id, "GPT-4o".to_string());

        let json = serde_json::to_value(&select_option).unwrap();
        assert_eq!(json["value"], "gpt-4o");
        assert_eq!(json["name"], "GPT-4o");
    }

    #[test]
    fn test_build_session_config_options_populates_options() {
        let modes = vec![
            agent_client_protocol::schema::v1::SessionMode::new(
                agent_client_protocol::schema::v1::SessionModeId::new("ask"),
                "Ask",
            ),
            agent_client_protocol::schema::v1::SessionMode::new(
                agent_client_protocol::schema::v1::SessionModeId::new("default"),
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
        let result =
            build_session_config_options("ask", "gpt-4o", "auto", &modes, &model_options, None);
        assert!(result.is_ok(), "Expected Ok, got Err: {:?}", result.err());

        let config_options = result.unwrap();
        assert_eq!(config_options.len(), 3);

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
        let modes = vec![agent_client_protocol::schema::v1::SessionMode::new(
            agent_client_protocol::schema::v1::SessionModeId::new("ask"),
            "Ask",
        )];
        let result = build_session_config_options("ask", "", "auto", &modes, &[], None);
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
        let modes = vec![agent_client_protocol::schema::v1::SessionMode::new(
            agent_client_protocol::schema::v1::SessionModeId::new("ask"),
            "Ask",
        )];
        let model_options = vec![ModelOption {
            id: "openai/gpt-4o".to_string(),
            name: "openai/gpt-4o".to_string(),
            provider: "openai".to_string(),
        }];

        let result = build_set_session_config_option_response(
            "ask",
            "gpt-4o",
            "auto",
            &modes,
            &model_options,
            None,
        );
        assert!(result.is_ok());

        let response = result.unwrap();
        let json = serde_json::to_value(&response).unwrap();
        assert!(json["configOptions"].is_array());
    }

    #[test]
    fn test_session_config_value_as_id_accepts_value_id_only() {
        let value_id = SessionConfigOptionValue::value_id("ask");
        assert_eq!(
            session_config_value_as_id(&value_id).as_deref(),
            Some("ask")
        );

        let boolean = SessionConfigOptionValue::boolean(true);
        assert!(session_config_value_as_id(&boolean).is_none());
    }

    #[test]
    fn test_normalize_current_model_for_acp_default() {
        let options = vec![ModelOption {
            id: "default".to_string(),
            name: "(default)".to_string(),
            provider: String::new(),
        }];
        assert_eq!(
            normalize_current_model_for_acp("default", &options),
            "default"
        );
        assert_eq!(normalize_current_model_for_acp("", &options), "default");
    }

    #[test]
    fn test_normalize_current_model_for_acp_specific_model() {
        let options = vec![
            ModelOption {
                id: "default".to_string(),
                name: "(default)".to_string(),
                provider: String::new(),
            },
            ModelOption {
                id: "openai/gpt-4o".to_string(),
                name: "openai/gpt-4o".to_string(),
                provider: "openai".to_string(),
            },
        ];
        assert_eq!(
            normalize_current_model_for_acp("openai/gpt-4o", &options),
            "openai/gpt-4o"
        );
    }

    #[test]
    fn test_build_session_config_options_includes_default() {
        let modes = vec![agent_client_protocol::schema::v1::SessionMode::new(
            agent_client_protocol::schema::v1::SessionModeId::new("ask"),
            "Ask",
        )];
        let model_options = vec![
            ModelOption {
                id: "default".to_string(),
                name: "(default)".to_string(),
                provider: String::new(),
            },
            ModelOption {
                id: "openai/gpt-4o".to_string(),
                name: "openai/gpt-4o".to_string(),
                provider: "openai".to_string(),
            },
        ];

        let result =
            build_session_config_options("ask", "default", "auto", &modes, &model_options, None);
        assert!(result.is_ok());

        let config_options = result.unwrap();
        let json = serde_json::to_value(&config_options).unwrap();
        let model_config = json
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c.get("id").and_then(|v| v.as_str()) == Some("model"))
            .unwrap();
        let options = model_config.get("options").unwrap().as_array().unwrap();
        assert_eq!(options[0].get("value").unwrap().as_str(), Some("default"));
        assert_eq!(
            model_config.get("currentValue").unwrap().as_str(),
            Some("default")
        );
    }

    #[test]
    fn test_build_acp_usage_empty() {
        let acc = Mutex::new(TurnUsage::default());
        assert!(build_acp_usage(&acc).is_none());
    }

    #[test]
    fn test_build_acp_usage_populated() {
        let acc = Mutex::new(TurnUsage {
            input_tokens: 1000,
            output_tokens: 500,
            total_tokens: 1500,
            cached_tokens: 200,
        });
        let usage = build_acp_usage(&acc).expect("usage should be Some");
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.total_tokens, 1500);
        assert_eq!(usage.cached_read_tokens, Some(200));
        assert_eq!(usage.thought_tokens, None);
    }

    #[test]
    fn test_capture_turn_usage_accumulates() {
        use stream_event::StreamEvent;
        use ReActState;

        let acc = Arc::new(Mutex::new(TurnUsage::default()));

        let ev1 = TypedAnyStreamEvent::React(StreamEvent::<ReActState>::TurnFinish {
            reason: "stop".to_string(),
            usage: stream_event::Usage {
                input: 500,
                output: 100,
                reasoning: None,
                cache_read: Some(50),
                cache_write: None,
            },
        });
        let ev2 = TypedAnyStreamEvent::React(StreamEvent::<ReActState>::TurnFinish {
            reason: "stop".to_string(),
            usage: stream_event::Usage {
                input: 800,
                output: 200,
                reasoning: None,
                cache_read: None,
                cache_write: None,
            },
        });

        capture_turn_usage(&ev1, &acc);
        capture_turn_usage(&ev2, &acc);

        let usage = build_acp_usage(&acc).expect("usage should be Some");
        assert_eq!(usage.input_tokens, 1300);
        assert_eq!(usage.output_tokens, 300);
        assert_eq!(usage.total_tokens, 1600);
        assert_eq!(usage.cached_read_tokens, Some(50));
    }

    #[test]
    fn test_capture_turn_usage_ignores_non_usage() {
        use stream_event::StreamEvent;
        use ReActState;

        let acc = Arc::new(Mutex::new(TurnUsage::default()));

        let ev = TypedAnyStreamEvent::React(StreamEvent::<ReActState>::TaskStart {
            node_id: "test".to_string(),
            namespace: None,
        });
        capture_turn_usage(&ev, &acc);

        assert!(build_acp_usage(&acc).is_none());
    }

    // ── initialize capability unit tests ──────────────────────────────
    //
    // These tests exercise the `initialize` handler directly (no process
    // spawn) and assert the returned `agentCapabilities` structure.
    // The corresponding e2e smoke test in `tests/e2e_mega.rs` only verifies
    // that the binary spawns and returns a valid initialize response.

    fn assert_mcp_caps(resp: &InitializeResponse) {
        let json = serde_json::to_value(&resp.agent_capabilities).expect("serialize");
        let mcp = json
            .get("mcpCapabilities")
            .expect("agentCapabilities.mcpCapabilities must be present");
        assert_eq!(
            mcp.get("http").and_then(serde_json::Value::as_bool),
            Some(true),
            "mcpCapabilities.http must be true"
        );
        assert_eq!(
            mcp.get("sse").and_then(serde_json::Value::as_bool),
            Some(false),
            "mcpCapabilities.sse must be false"
        );
    }

    fn assert_prompt_caps(resp: &InitializeResponse) {
        let json = serde_json::to_value(&resp.agent_capabilities).expect("serialize");
        let prompts = json
            .get("promptCapabilities")
            .expect("agentCapabilities.promptCapabilities must be present");
        assert_eq!(
            prompts.get("image").and_then(serde_json::Value::as_bool),
            Some(true),
            "promptCapabilities.image must be true"
        );
        assert_eq!(
            prompts.get("audio").and_then(serde_json::Value::as_bool),
            Some(true),
            "promptCapabilities.audio must be true"
        );
        assert_eq!(
            prompts
                .get("embeddedContext")
                .and_then(serde_json::Value::as_bool),
            Some(true),
            "promptCapabilities.embeddedContext must be true"
        );
    }

    #[tokio::test]
    async fn test_initialize_returns_mcp_capabilities() {
        let agent = LoomAcpAgent::new().expect("agent");
        let req = InitializeRequest::new(1.into());
        let resp = agent.initialize(req).await.expect("initialize");
        assert!(resp.protocol_version >= 1.into());
        assert_mcp_caps(&resp);
    }

    #[tokio::test]
    async fn test_initialize_returns_prompt_capabilities() {
        let agent = LoomAcpAgent::new().expect("agent");
        let req = InitializeRequest::new(1.into());
        let resp = agent.initialize(req).await.expect("initialize");
        assert_prompt_caps(&resp);
    }

    #[tokio::test]
    async fn test_initialize_accepts_client_mcp_capabilities() {
        let agent = LoomAcpAgent::new().expect("agent");
        let req = InitializeRequest::new(1.into()).client_info(
            agent_client_protocol::schema::v1::Implementation::new(
                "test-client".to_string(),
                "0.1.0".to_string(),
            ),
        );
        let resp = agent.initialize(req).await.expect("initialize");
        assert_mcp_caps(&resp);
    }

    #[tokio::test]
    async fn test_initialize_accepts_client_prompt_capabilities() {
        let agent = LoomAcpAgent::new().expect("agent");
        let req = InitializeRequest::new(1.into()).client_info(
            agent_client_protocol::schema::v1::Implementation::new(
                "test-client".to_string(),
                "0.1.0".to_string(),
            ),
        );
        let resp = agent.initialize(req).await.expect("initialize");
        assert_prompt_caps(&resp);
    }

    #[tokio::test]
    async fn initialize_advertises_registered_session_lifecycle_methods() {
        let agent = LoomAcpAgent::new().expect("agent");
        let resp = agent
            .initialize(InitializeRequest::new(1.into()))
            .await
            .expect("initialize");
        let session = resp.agent_capabilities.session_capabilities;
        assert!(session.list.is_some());
        assert!(session.resume.is_some());
        assert!(session.close.is_some());
        assert!(session.delete.is_some());
    }
}
