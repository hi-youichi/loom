//! Run orchestration for agent patterns (ReAct, ToT, GoT, DUP).
//!
//! Most agent functionality has moved to loom-agent crate.
//! This module contains only what's needed for loom infrastructure.

mod profile;

use crate::skill::SkillRegistry;
use crate::helve::env_context::ProjectInfo;
use crate::helve::EnvContext;
use crate::{
    assemble_react_system_prompt, to_react_build_config, HelveConfig, ReactBuildConfig,
    ReactPromptInputs,
};
use std::path::PathBuf;
use std::sync::Arc;

pub use profile::{
    list_available_profiles, load_profile_from_options, resolve_profile, AgentProfile,
    ProfileError, ProfileSource, ProfileSummary,
};

/// Default working folder when not set (current directory).
pub const DEFAULT_WORKING_FOLDER: &str = ".";

/// Metadata about the agent profile that was resolved for a run.
#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub name: String,
    pub description: Option<String>,
    pub source: ProfileSource,
}

/// Resolved model + provider configuration from a model string like "openai/gpt-4o".
#[derive(Debug, Clone, Default)]
pub struct ResolvedModelConfig {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
}

// TODO: Agent types moved to loom-agent crate
// Stub RunOptions for loom infrastructure that only needs base_url/api_key/model/etc
pub struct RunOptions {
    pub message: crate::UserContent,
    pub working_folder: Option<PathBuf>,
    pub session_id: Option<String>,
    pub cancellation: Option<crate::RunCancellation>,
    pub thread_id: Option<String>,
    pub agent: Option<String>,
    pub verbose: bool,
    pub got_adaptive: bool,
    pub display_max_len: usize,
    pub output_json: bool,
    pub model: Option<String>,
    pub mcp_config_path: Option<PathBuf>,
    pub output_timestamp: bool,
    pub dry_run: bool,
    pub debug_llm: bool,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
    pub any_stream_event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
    pub bash_executor: Option<Arc<dyn loom_tools::CommandExecutor>>,
    pub extra_tools: Option<Arc<Vec<Arc<dyn crate::tools::Tool>>>>,
    pub acp_session_id: Option<String>,
    pub force_compact: bool,
    pub chat_id: Option<i64>,
    pub worktree: bool,
}

impl std::fmt::Debug for RunOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunOptions")
            .field("message", &"<UserContent>")
            .field("working_folder", &self.working_folder)
            .field("model", &self.model)
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("thread_id", &self.thread_id)
            .field("dry_run", &self.dry_run)
            .finish_non_exhaustive()
    }
}

impl Clone for RunOptions {
    fn clone(&self) -> Self {
        Self {
            message: self.message.clone(),
            working_folder: self.working_folder.clone(),
            session_id: self.session_id.clone(),
            cancellation: self.cancellation.clone(),
            thread_id: self.thread_id.clone(),
            agent: self.agent.clone(),
            verbose: self.verbose,
            got_adaptive: self.got_adaptive,
            display_max_len: self.display_max_len,
            output_json: self.output_json,
            model: self.model.clone(),
            mcp_config_path: self.mcp_config_path.clone(),
            output_timestamp: self.output_timestamp,
            dry_run: self.dry_run,
            debug_llm: self.debug_llm,
            provider: self.provider.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            provider_type: self.provider_type.clone(),
            any_stream_event_sender: self.any_stream_event_sender.clone(),
            bash_executor: self.bash_executor.clone(),
            extra_tools: self.extra_tools.clone(),
            acp_session_id: self.acp_session_id.clone(),
            force_compact: self.force_compact,
            chat_id: self.chat_id,
            worktree: self.worktree,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCmd {
    React,
    Dup,
    Tot,
    Got,
}

#[derive(Debug, Clone)]
pub enum RunCompletion {
    Finished(AgentRunResult),
    Cancelled,
}

#[derive(Debug, Clone)]
pub enum RunError {
    LlmError(String),
    ToolError(String),
    Other(String),
}

#[derive(Debug, Clone)]
pub enum AnyStreamEvent {
    React(crate::stream::StreamEvent<crate::ReActState>),
    Dup(crate::stream::StreamEvent<StubDupState>),
    Tot(crate::stream::StreamEvent<StubTotState>),
    Got(crate::stream::StreamEvent<StubGotState>),
}

// Stub agent state types for AnyStreamEvent (these will be available from loom-agent)
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StubDupState {
    pub core: crate::ReActState,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StubTotState {
    pub core: crate::ReActState,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StubGotState {
    pub input_message: String,
}

#[derive(Debug, Clone)]
pub struct AgentRunResult {
    pub reply: String,
    pub reasoning_content: Option<String>,
}

// Re-export from active_operation module
pub use crate::active_operation::{ActiveOperation, ActiveOperationCanceller, ActiveOperationKind, RunCancellation};

// Re-export from loom-types for use in loom infrastructure
pub use loom_types::state::ReActState;

const AGENTS_MD_FILE: &str = "AGENTS.md";

/// Reads AGENTS.md from current directory and optionally from working_folder.
pub fn load_memory_prompt() -> Option<String> {
    let memory_dir = env_config::home::loom_home().join("data").join("memory");
    let files = [
        ("FACTS.md", "## Facts"),
        ("PROJECT.md", "## Project"),
        ("USER.md", "## User"),
    ];
    let mut parts = Vec::new();
    for (filename, header) in &files {
        let path = memory_dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                parts.push(format!("{}\n{}", header, content));
            }
        }
    }
    if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
}

pub fn load_agents_md(working_folder: Option<&PathBuf>) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let cwd_canon = cwd.canonicalize().unwrap_or(cwd.clone());
    let cwd_agents = std::fs::read_to_string(cwd.join(AGENTS_MD_FILE))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string());
    let work_agents = working_folder
        .filter(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()) != cwd_canon)
        .and_then(|p| std::fs::read_to_string(p.join(AGENTS_MD_FILE)).ok())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string());
    match (cwd_agents, work_agents) {
        (Some(c), Some(w)) => Some(format!("{}\n\n{}", c, w)),
        (Some(c), None) => Some(c),
        (None, Some(w)) => Some(w),
        (None, None) => None,
    }
}

/// `role_setting` from the resolved agent profile only (trimmed non-empty content).
fn role_content_from_profile(profile_role: Option<String>) -> Option<String> {
    profile_role.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    })
}

/// Builds HelveConfig and ReactBuildConfig from RunOptions.
/// Returns an optional `ResolvedAgent` describing which agent profile was loaded.
pub fn build_helve_config(
    opts: &RunOptions,
) -> (HelveConfig, ReactBuildConfig, Option<ResolvedAgent>) {
    let loaded = load_profile_from_options(opts);
    let resolved_agent = loaded.as_ref().map(|(p, source)| ResolvedAgent {
        name: p.name.clone(),
        description: p.description.clone(),
        source: source.clone(),
    });
    let profile = loaded.map(|(p, _)| p);
    let mut effective_opts = opts.clone();
    apply_model_provider_resolution(&mut effective_opts);
    if let Some(ref p) = profile {
        apply_profile_to_run_options(p, &mut effective_opts);
    }

    let mut base = ReactBuildConfig::from_env();
    base.dry_run = effective_opts.dry_run;
    {
        let model = effective_opts.model.as_deref();
        let provider = effective_opts.provider.as_deref();
        let has_provider_prefix = model.is_some_and(|m| m.contains('/'));
        base.model = match (model, provider) {
            (Some(m), Some(p)) if !has_provider_prefix => Some(format!("{}/{}", p, m)),
            (Some(m), _) => Some(m.to_string()),
            _ => None,
        };
    }

    // Provider configuration from RunOptions (used by ACP to specify provider-specific settings)
    if let Some(ref url) = effective_opts.base_url {
        base.openai_base_url = Some(url.clone());
    }
    if let Some(ref key) = effective_opts.api_key {
        base.openai_api_key = Some(key.clone());
    }
    if let Some(ref t) = effective_opts.provider_type {
        base.llm_provider = Some(t.clone());
    }

    if let Some(ref prof) = profile {
        if let Some(t) = prof.model.as_ref().and_then(|m| m.temperature) {
            base.openai_temperature = Some(t.to_string());
        }
        let model_explicitly_set = effective_opts.model.is_some() || opts.model.is_some();
        if !model_explicitly_set {
            if let Some(tier) = prof.model.as_ref().and_then(|m| m.tier) {
                base.model_tier = Some(tier);
                tracing::debug!(
                    tier = ?tier,
                    "No explicit model specified, applying profile tier configuration"
                );
            }
        } else {
            tracing::debug!(
                model = ?effective_opts.model,
                "Model explicitly specified, skipping profile tier configuration to avoid override"
            );
        }
    }

    let working_folder = effective_opts
        .working_folder
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORKING_FOLDER));

    let profile_role = profile
        .as_ref()
        .and_then(|p| p.role.as_ref().and_then(|r| r.content.clone()));

    // MCP config: CLI > profile > LOOM_MCP_CONFIG_PATH > discover
    let override_path = effective_opts.mcp_config_path.clone().or_else(|| {
        std::env::var("LOOM_MCP_CONFIG_PATH")
            .ok()
            .map(PathBuf::from)
    });
    if let Some(path) =
        env_config::discover_mcp_config_path(override_path.as_deref(), Some(&working_folder))
    {
        match env_config::load_mcp_config_from_path(&path) {
            Ok(servers) => base.mcp_servers = Some(servers),
            Err(e) => tracing::warn!(path = %path.display(), "failed to load mcp config: {}", e),
        }
    }

    let skill_registry = {
        let extra_dirs: Vec<PathBuf> = profile
            .as_ref()
            .and_then(|p| p.skills.as_ref())
            .and_then(|s| s.dirs.as_ref())
            .map(|dirs| dirs.iter().map(PathBuf::from).collect())
            .unwrap_or_default();
        let mut registry = SkillRegistry::discover(&working_folder, &extra_dirs);
        if let Some(ref p) = profile {
            if let Some(ref src) = p.source_dir {
                registry.add_agent_skills(&src.join("skills"));
            }
            if let Some(ref sc) = p.skills {
                registry.apply_filters(sc.enabled.as_deref(), sc.disabled.as_deref());
            }
        }
        let arc = Arc::new(registry);
        let prompt = arc.available_skills_prompt();
        (arc, prompt)
    };

    let skills_prompt = if skill_registry.1.is_empty() {
        None
    } else {
        let mut prompt = skill_registry.1.clone();
        if let Some(ref p) = profile {
            if let Some(preload) = p.skills.as_ref().and_then(|s| s.preload.as_ref()) {
                let mut buf = String::new();
                for name in preload {
                    if let Ok(content) = skill_registry.0.load_skill(name) {
                        buf.push_str(&format!(
                            "<skill name=\"{}\">\n{}</skill>\n",
                            name, content
                        ));
                    }
                }
                if !buf.is_empty() {
                    prompt.push_str(&format!(
                        "\n\n<preloaded_skills>\n{}</preloaded_skills>",
                        buf
                    ));
                }
            }
        }
        Some(prompt)
    };

    let agent_instructions = role_content_from_profile(profile_role);

    tracing::trace!(
        agent_instructions_len = agent_instructions.as_ref().map(|s| s.len()),
        skills_prompt_len = skills_prompt.as_ref().map(|s| s.len()),
        "agent prompt",
    );

    let helve = HelveConfig {
        working_folder: Some(working_folder.clone()),
        thread_id: effective_opts.thread_id.clone(),
        user_id: base.user_id.clone(),
        approval_policy: None,
        role_setting: agent_instructions,
        agents_md: load_agents_md(Some(&working_folder)),
        system_prompt_override: None,
        skills_prompt,
        memory_prompt: load_memory_prompt(),
        env_context: Some({
            let mut ctx = EnvContext::detect().with_project(
                ProjectInfo::detect(&working_folder),
            );
            if let Some(cid) = effective_opts.chat_id {
                ctx = ctx.with_chat_id(cid);
            }
            ctx
        }),
    };
    let mut config = to_react_build_config(&helve, base);
    config.skill_registry = Some(skill_registry.0);
    config.max_sub_agent_depth = profile
        .as_ref()
        .and_then(|p| p.behavior.as_ref())
        .and_then(|b| b.max_sub_agent_depth);

    // Builtin tool filter from agent profile
    if let Some(ref prof) = profile {
        if let Some(ref tools) = prof.tools {
            if let Some(ref builtin) = tools.builtin {
                let filter = loom_types::config::BuiltinToolFilter {
                    enabled: builtin.enabled.clone(),
                    disabled: builtin.disabled.clone(),
                };
                if !filter.is_noop() {
                    config.builtin_tool_filter = Some(filter);
                }
            }
        }
    }

    (helve, config, resolved_agent)
}

/// Builds a `ReactBuildConfig` for a sub-agent from a resolved profile and
/// the parent agent's config. The parent config provides LLM credentials,
/// provider, and other environment-derived settings; the profile can override
/// model name, working_folder, MCP config, and system prompt.
pub fn build_config_from_profile(
    profile: &AgentProfile,
    parent_config: &ReactBuildConfig,
    working_folder_override: Option<&std::path::Path>,
) -> ReactBuildConfig {
    let mut config = parent_config.clone();

    tracing::debug!(
        profile_name = %profile.name,
        parent_model = ?parent_config.model,
        parent_model_tier = ?parent_config.model_tier,
        parent_provider = ?parent_config.llm_provider,
        "Building config from profile with parent model configuration"
    );

    if let Some(ref model) = profile.model {
        tracing::debug!(
            profile_name = %profile.name,
            profile_model_name = ?model.name,
            profile_model_tier = ?model.tier,
            profile_model_temperature = ?model.temperature,
            "Profile contains model configuration"
        );

        if let Some(ref name) = model.name {
            tracing::info!(
                profile_name = %profile.name,
                old_model = ?config.model,
                new_model = %name,
                "Overriding model from profile"
            );
            config.model = Some(name.clone());
        }
        if let Some(tier) = model.tier {
            tracing::info!(
                profile_name = %profile.name,
                old_tier = ?config.model_tier,
                new_tier = ?tier,
                "Overriding model_tier from profile"
            );
            config.model_tier = Some(tier);

            tracing::debug!(
                profile_name = %profile.name,
                tier = ?tier,
                preserved_provider = ?config.llm_provider,
                "Clearing inherited model/api fields for clean tier resolution (preserving llm_provider)"
            );
            config.parent_model_hint = config.model.take();
            config.openai_base_url = None;
            config.openai_api_key = None;
        }
        if let Some(t) = model.temperature {
            tracing::debug!(
                profile_name = %profile.name,
                old_temperature = ?config.openai_temperature,
                new_temperature = t,
                "Setting temperature from profile"
            );
            config.openai_temperature = Some(t.to_string());
        }
    } else {
        tracing::debug!(
            profile_name = %profile.name,
            "Profile has no model configuration, inheriting from parent"
        );
    }

    if let Some(wf) = working_folder_override {
        config.working_folder = Some(wf.to_path_buf());
    } else if let Some(ref env) = profile.environment {
        if let Some(ref wf) = env.working_folder {
            config.working_folder = Some(wf.clone());
        }
    }

    let working_folder = config
        .working_folder
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORKING_FOLDER));

    // MCP config from profile
    if let Some(ref tools) = profile.tools {
        if let Some(ref mcp) = tools.mcp {
            if let Some(ref mcp_path) = mcp.config {
                if let Some(path) = env_config::discover_mcp_config_path(
                    Some(mcp_path.as_path()),
                    Some(&working_folder),
                ) {
                    match env_config::load_mcp_config_from_path(&path) {
                        Ok(servers) => config.mcp_servers = Some(servers),
                        Err(e) => tracing::warn!(
                            path = %path.display(),
                            "sub-agent: failed to load mcp config: {}", e
                        ),
                    }
                }
            }
        }
    }

    // System prompt from profile role / AGENTS.md uses the same assembler as top-level runs.
    let role_setting =
        role_content_from_profile(profile.role.as_ref().and_then(|r| r.content.clone()));
    let agents_md = load_agents_md(Some(&working_folder));
    if role_setting.is_some() || agents_md.is_some() {
        let prompt_inputs = ReactPromptInputs {
            base_prompt_override: config.system_prompt.take(),
            role_setting,
            agents_md,
            ..Default::default()
        };
        config.system_prompt = Some(assemble_react_system_prompt(&prompt_inputs));
    }

    // Skill registry for sub-agent
    let extra_dirs: Vec<PathBuf> = profile
        .skills
        .as_ref()
        .and_then(|s| s.dirs.as_ref())
        .map(|dirs| dirs.iter().map(PathBuf::from).collect())
        .unwrap_or_default();
    let mut registry = SkillRegistry::discover(&working_folder, &extra_dirs);
    if let Some(ref src) = profile.source_dir {
        registry.add_agent_skills(&src.join("skills"));
    }
    if let Some(ref sc) = profile.skills {
        registry.apply_filters(sc.enabled.as_deref(), sc.disabled.as_deref());
    }
    config.skill_registry = Some(Arc::new(registry));

    config.max_sub_agent_depth = profile
        .behavior
        .as_ref()
        .and_then(|b| b.max_sub_agent_depth)
        .or(parent_config.max_sub_agent_depth);

    // Builtin tool filter from profile
    if let Some(ref tools) = profile.tools {
        if let Some(ref builtin) = tools.builtin {
            let filter = loom_types::config::BuiltinToolFilter {
                enabled: builtin.enabled.clone(),
                disabled: builtin.disabled.clone(),
            };
            if !filter.is_noop() {
                config.builtin_tool_filter = Some(filter);
            }
        }
    }

    tracing::debug!(
        profile_name = %profile.name,
        final_model = ?config.model,
        final_model_tier = ?config.model_tier,
        final_provider = ?config.llm_provider,
        final_temperature = ?config.openai_temperature,
        parent_model = ?parent_config.model,
        parent_model_tier = ?parent_config.model_tier,
        parent_provider = ?parent_config.llm_provider,
        "Final configuration built from profile with model inheritance details"
    );

    config
}

/// Resolve a model string (e.g. "openai/gpt-4o", "gpt-4o") into model id + provider config.
pub async fn resolve_model_config(model_str: Option<&str>) -> ResolvedModelConfig {
    let Some(model_str) = model_str else {
        tracing::debug!("No model string provided, using default configuration");
        return ResolvedModelConfig::default();
    };
    if model_str.is_empty() {
        tracing::debug!("Empty model string provided, using default configuration");
        return ResolvedModelConfig::default();
    }

    tracing::info!("🔍 Resolving model configuration for: {}", model_str);

    let providers: Vec<crate::llm::ProviderConfig> =
        crate::provider::load_provider_configs().unwrap_or_default();

    // 1. Try ModelRegistry first
    tracing::debug!("🔎 Searching ModelRegistry for: {}", model_str);
    if let Some(entry) = crate::llm::ModelRegistry::global()
        .get_model(model_str, &providers)
        .await
    {
        tracing::info!(
            "✅ Found model in registry: {} from provider {}",
            entry.id,
            entry.provider
        );
        return ResolvedModelConfig {
            model: Some(entry.id.clone()),
            provider: Some(entry.provider.clone()),
            base_url: entry.base_url,
            api_key: entry.api_key,
            provider_type: entry.provider_type,
        };
    }

    tracing::warn!("❌ Model not found in registry, trying provider/model split");

    // 2. Try "provider/model" split
    if let Some((provider_name, model_id)) = model_str.split_once('/') {
        let actual_model_id = model_id
            .rsplit_once('/')
            .map(|(_, m)| m)
            .unwrap_or(model_id);
        tracing::debug!(
            provider = %provider_name,
            model_id = %model_id,
            actual_model_id = %actual_model_id,
            "Model not in registry, loading provider config"
        );
        let provider_cfg = env_config::load_full_config("loom")
            .ok()
            .and_then(|c| c.providers.into_iter().find(|p| p.name == provider_name));
        if let Some(p) = provider_cfg {
            tracing::info!(
                "✅ Resolved model from provider config: {} from provider {}",
                model_str,
                p.name
            );
            return ResolvedModelConfig {
                model: Some(model_str.to_string()),
                provider: Some(p.name),
                base_url: p.base_url,
                api_key: p.api_key,
                provider_type: p.provider_type,
            };
        } else {
            tracing::warn!(
                "⚠️  Provider '{}' not found in config, using bare model",
                provider_name
            );
        }
    }

    // 3. Bare model id — backward compat
    tracing::info!("🔧 Using bare model ID: {}", model_str);
    ResolvedModelConfig {
        model: Some(model_str.to_string()),
        ..Default::default()
    }
}

fn apply_model_provider_resolution(opts: &mut RunOptions) {
    tracing::debug!(
        "🔧 apply_model_provider_resolution called with model: {:?}, provider: {:?}",
        opts.model,
        opts.provider
    );

    if opts.model.is_none() && opts.provider.is_none() {
        tracing::debug!("⏭️  Skipping model resolution - no model or provider specified");
        return;
    }

    let provider_only = opts.provider.clone();
    let raw_model = match opts.model.as_deref() {
        Some(m) => m.to_string(),
        None => {
            tracing::debug!(
                "🔧 No model specified, resolving provider only: {:?}",
                provider_only
            );
            resolve_provider_fields_into_opts(provider_only.as_deref(), opts);
            return;
        }
    };

    tracing::debug!("🔧 Processing model: {}", raw_model);

    // Validate provider/model format
    if raw_model.is_empty() {
        tracing::warn!("❌ Model name cannot be empty");
        opts.model = None;
        return;
    }

    let (resolved_provider, model_name) = if let Some((p, m)) = raw_model.split_once('/') {
        // Validate provider and model parts
        if p.is_empty() {
            tracing::warn!("❌ Provider name in 'provider/model' format cannot be empty");
            opts.model = None;
            return;
        }
        if m.is_empty() {
            tracing::warn!("❌ Model name in 'provider/model' format cannot be empty");
            opts.model = None;
            return;
        }
        tracing::debug!(
            "✅ Parsed provider/model format: provider={}, model={}",
            p,
            m
        );
        (Some(p.to_string()), m.to_string())
    } else {
        // Bare model name - validate it's not just whitespace
        if raw_model.trim().is_empty() {
            tracing::warn!("❌ Model name cannot be empty or whitespace only");
            opts.model = None;
            return;
        }
        tracing::debug!("✅ Using bare model name: {}", raw_model.trim());
        (None, raw_model.trim().to_string())
    };

    let effective_provider = provider_only.as_deref().or(resolved_provider.as_deref());
    opts.model = Some(model_name.clone());
    if opts.provider.is_none() {
        if let Some(ref p) = resolved_provider {
            opts.provider = Some(p.clone());
        }
    }

    tracing::info!(
        "🎯 Final resolution: model_name={}, provider={:?}",
        model_name,
        effective_provider
    );

    if let Some(name) = effective_provider {
        let name = name.to_string();
        resolve_provider_fields_into_opts(Some(name.as_str()), opts);
    }
}

fn resolve_provider_fields_into_opts(provider_name: Option<&str>, opts: &mut RunOptions) {
    let Some(name) = provider_name else { return };

    let full_config = match env_config::load_full_config("loom") {
        Ok(c) => c,
        Err(_) => return,
    };

    let provider = match full_config
        .providers
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
    {
        Some(p) => p,
        None => {
            tracing::warn!(
                provider = name,
                "Provider not found in config.toml [[providers]]"
            );
            return;
        }
    };

    if opts.api_key.is_none() {
        if let Some(ref key) = provider.api_key {
            opts.api_key = Some(key.clone());
        }
    }
    if opts.base_url.is_none() {
        if let Some(ref url) = provider.base_url {
            opts.base_url = Some(url.clone());
        }
    }
    if opts.provider_type.is_none() {
        if let Some(ref t) = provider.provider_type {
            opts.provider_type = Some(t.clone());
        }
    }
}

fn apply_profile_to_run_options(profile: &AgentProfile, opts: &mut RunOptions) {
    if let Some(ref tools) = profile.tools {
        if let Some(ref mcp) = tools.mcp {
            if let Some(ref config) = mcp.config {
                if opts.mcp_config_path.is_none() {
                    opts.mcp_config_path = Some(config.clone());
                }
            }
        }
    }
    if let Some(ref model) = profile.model {
        if let Some(ref name) = model.name {
            if opts.model.is_none() {
                opts.model = Some(name.clone());
            }
        }
    }
    if let Some(ref env) = profile.environment {
        if opts.working_folder.is_none() {
            opts.working_folder = env.working_folder.clone();
        }
        if opts.thread_id.is_none() {
            opts.thread_id = env.thread_id.clone();
        }
    }
}