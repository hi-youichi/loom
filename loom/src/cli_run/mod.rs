//! Run orchestration for agent patterns (ReAct, ToT, GoT, DUP).
//!
//! Builds HelveConfig and ReactBuildConfig, then invokes the corresponding runner.
//! Used by both cli (local) and loom serve (remote).

mod agent;
mod profile;

pub use agent::{
    run_agent, run_agent_with_llm_override, run_agent_with_options, run_agent_with_provider,
    ActiveOperation, ActiveOperationCanceller, ActiveOperationKind, AgentRunResult, AnyRunner,
    AnyStreamEvent, RunCancellation, RunCmd, RunCompletion, RunError, RunOptions,
};

use crate::skill::SkillRegistry;
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

/// Metadata about the agent profile that was resolved for a run.
#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub name: String,
    pub description: Option<String>,
    pub source: ProfileSource,
}

/// Default working folder when not set (current directory).
pub const DEFAULT_WORKING_FOLDER: &str = ".";

const AGENTS_MD_FILE: &str = "AGENTS.md";

/// Reads AGENTS.md from current directory and optionally from working_folder.
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
    if let Some(ref m) = effective_opts.model {
        base.model = Some(m.clone());
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
                            "<skill name=\"{}\">\n{}\n</skill>\n",
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
    };
    let mut config = to_react_build_config(&helve, base);
    config.skill_registry = Some(skill_registry.0);
    config.max_sub_agent_depth = profile
        .as_ref()
        .and_then(|p| p.behavior.as_ref())
        .and_then(|b| b.max_sub_agent_depth);
    (helve, config, resolved_agent)
}

/// Builds a `ReactBuildConfig` for a sub-agent from a resolved profile and
/// the parent agent's config. The parent config provides LLM credentials,
/// provider, and other environment-derived settings; the profile can override
/// model name, working_folder, MCP config, and system prompt.
///
/// Used by `InvokeAgentTool` to construct a child `ReactRunner` at runtime.
pub fn build_config_from_profile(
    profile: &AgentProfile,
    parent_config: &ReactBuildConfig,
    working_folder_override: Option<&std::path::Path>,
) -> ReactBuildConfig {
    let mut config = parent_config.clone();

    if let Some(ref model) = profile.model {
        if let Some(ref name) = model.name {
            config.model = Some(name.clone());
        }
        if let Some(t) = model.temperature {
            config.openai_temperature = Some(t.to_string());
        }
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

    config
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

/// Resolve a model string (e.g. "openai/gpt-4o", "gpt-4o") into model id + provider config.
///
/// Resolution order:
/// 1. `ModelRegistry::get_model()` — full registry lookup
/// 2. `"provider/model"` split — fallback to config file provider lookup
/// 3. Bare model id — no provider resolution (backward compat)
pub async fn resolve_model_config(model_str: Option<&str>) -> ResolvedModelConfig {
    let Some(model_str) = model_str else {
        return ResolvedModelConfig::default();
    };
    if model_str.is_empty() {
        return ResolvedModelConfig::default();
    }

    let providers: Vec<crate::llm::ProviderConfig> = match env_config::load_full_config("loom") {
        Ok(config) => config
            .providers
            .into_iter()
            .map(|p| crate::llm::ProviderConfig {
                name: p.name,
                base_url: p.base_url,
                api_key: p.api_key,
                provider_type: p.provider_type,
            })
            .collect(),
        Err(_) => vec![],
    };

    // 1. Try ModelRegistry first
    if let Some(entry) = crate::llm::ModelRegistry::global()
        .get_model(model_str, &providers)
        .await
    {
        return ResolvedModelConfig {
            model: Some(entry.id.clone()),
            provider: Some(entry.provider.clone()),
            base_url: entry.base_url,
            api_key: entry.api_key,
            provider_type: entry.provider_type,
        };
    }

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
            return ResolvedModelConfig {
                model: Some(model_str.to_string()),
                provider: Some(p.name),
                base_url: p.base_url,
                api_key: p.api_key,
                provider_type: p.provider_type,
            };
        }
    }

    // 3. Bare model id — backward compat
    ResolvedModelConfig {
        model: Some(model_str.to_string()),
        ..Default::default()
    }
}

fn apply_model_provider_resolution(opts: &mut RunOptions) {
    if opts.model.is_none() && opts.provider.is_none() {
        return;
    }

    let provider_only = opts.provider.clone();
    let raw_model = match opts.model.as_deref() {
        Some(m) => m.to_string(),
        None => {
            resolve_provider_fields_into_opts(provider_only.as_deref(), opts);
            return;
        }
    };

    // Validate provider/model format
    if raw_model.is_empty() {
        tracing::warn!("Model name cannot be empty");
        opts.model = None;
        return;
    }

    let (resolved_provider, model_name) = if let Some((p, m)) = raw_model.split_once('/') {
        // Validate provider and model parts
        if p.is_empty() {
            tracing::warn!("Provider name in 'provider/model' format cannot be empty");
            opts.model = None;
            return;
        }
        if m.is_empty() {
            tracing::warn!("Model name in 'provider/model' format cannot be empty");
            opts.model = None;
            return;
        }
        (Some(p.to_string()), m.to_string())
    } else {
        // Bare model name - validate it's not just whitespace
        if raw_model.trim().is_empty() {
            tracing::warn!("Model name cannot be empty or whitespace only");
            opts.model = None;
            return;
        }
        (None, raw_model.trim().to_string())
    };

    let effective_provider = provider_only.as_deref().or(resolved_provider.as_deref());
    opts.model = Some(model_name);

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

    let provider = match full_config.providers.iter().find(|p| {
        p.name.eq_ignore_ascii_case(name)
    }) {
        Some(p) => p,
        None => {
            tracing::warn!(provider = name, "Provider not found in config.toml [[providers]]");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_run::profile::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn default_opts() -> RunOptions {
        use crate::message::UserContent;
        RunOptions {
            message: UserContent::Text(String::new()),
            working_folder: None,
            session_id: None,
            cancellation: None,
            thread_id: None,
            agent: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 120,
            output_json: false,
            model: None,
            mcp_config_path: None,
            output_timestamp: false,
            dry_run: false,
            provider: None,
            base_url: None,
            api_key: None,
            provider_type: None,
        }
    }

    #[test]
    fn load_agents_md_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::current_dir().ok();
        let _ = std::env::set_current_dir(dir.path());
        let result = load_agents_md(None);
        if let Some(d) = prev {
            let _ = std::env::set_current_dir(d);
        }
        assert!(result.is_none());
    }

    #[test]
    fn load_agents_md_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "# Agent rules").unwrap();
        let prev = std::env::current_dir().ok();
        let _ = std::env::set_current_dir(dir.path());
        let result = load_agents_md(None);
        if let Some(d) = prev {
            let _ = std::env::set_current_dir(d);
        }
        assert!(result.is_some());
        assert!(result.unwrap().contains("Agent rules"));
    }

    #[test]
    fn load_agents_md_with_working_folder() {
        let cwd = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("AGENTS.md"), "# Work agents").unwrap();
        let prev = std::env::current_dir().ok();
        let _ = std::env::set_current_dir(cwd.path());
        let wf = work.path().to_path_buf();
        let result = load_agents_md(Some(&wf));
        if let Some(d) = prev {
            let _ = std::env::set_current_dir(d);
        }
        assert!(result.is_some());
        assert!(result.unwrap().contains("Work agents"));
    }

    #[test]
    fn role_content_from_profile_whitespace_none() {
        assert!(role_content_from_profile(Some("  \n\t  ".to_string())).is_none());
    }

    #[test]
    fn role_content_from_profile_trims_and_returns() {
        assert_eq!(
            role_content_from_profile(Some("  hello  ".to_string())).as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn role_content_from_profile_none_in_none_out() {
        assert!(role_content_from_profile(None).is_none());
    }

    #[test]
    fn apply_profile_sets_model() {
        let profile = AgentProfile {
            model: Some(ModelConfig {
                name: Some("gpt-5".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut opts = default_opts();
        apply_profile_to_run_options(&profile, &mut opts);
        assert_eq!(opts.model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn apply_profile_does_not_override_existing_model() {
        let profile = AgentProfile {
            model: Some(ModelConfig {
                name: Some("gpt-5".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut opts = default_opts();
        opts.model = Some("gpt-4".to_string());
        apply_profile_to_run_options(&profile, &mut opts);
        assert_eq!(opts.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn apply_profile_sets_mcp_config() {
        let profile = AgentProfile {
            tools: Some(ToolsConfig {
                mcp: Some(McpConfig {
                    config: Some(PathBuf::from("./mcp.json")),
                    servers: None,
                }),
                builtin: None,
            }),
            ..Default::default()
        };
        let mut opts = default_opts();
        apply_profile_to_run_options(&profile, &mut opts);
        assert_eq!(opts.mcp_config_path, Some(PathBuf::from("./mcp.json")));
    }

    #[test]
    fn apply_profile_sets_environment() {
        let profile = AgentProfile {
            environment: Some(EnvironmentConfig {
                working_folder: Some(PathBuf::from("/custom/dir")),
                thread_id: Some("t-123".to_string()),
                user_id: None,
            }),
            ..Default::default()
        };
        let mut opts = default_opts();
        apply_profile_to_run_options(&profile, &mut opts);
        assert_eq!(opts.working_folder, Some(PathBuf::from("/custom/dir")));
        assert_eq!(opts.thread_id.as_deref(), Some("t-123"));
    }

    fn parent_config() -> ReactBuildConfig {
        let mut c = ReactBuildConfig::from_env();
        c.model = Some("parent-model".to_string());
        c
    }

    #[test]
    fn build_config_from_profile_minimal() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());

        let profile = AgentProfile::default();
        let parent = parent_config();
        let config = build_config_from_profile(&profile, &parent, None);
        assert_eq!(config.model.as_deref(), Some("parent-model"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn build_config_from_profile_overrides_model() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());

        let profile = AgentProfile {
            model: Some(ModelConfig {
                name: Some("child-model".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let config = build_config_from_profile(&profile, &parent_config(), None);
        assert_eq!(config.model.as_deref(), Some("child-model"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn build_config_from_profile_working_folder_override() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());

        let profile = AgentProfile::default();
        let wf = tempfile::tempdir().unwrap();
        let config = build_config_from_profile(&profile, &parent_config(), Some(wf.path()));
        assert_eq!(config.working_folder, Some(wf.path().to_path_buf()));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn build_config_from_profile_with_role() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = ENV_LOCK.lock().unwrap();
        let loom_home = tempfile::tempdir().unwrap();
        let prev = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", loom_home.path());

        let profile = AgentProfile {
            role: Some(RoleConfig {
                file: None,
                content: Some("You are a sub-agent.".to_string()),
            }),
            ..Default::default()
        };
        let config = build_config_from_profile(&profile, &parent_config(), None);
        assert!(config.system_prompt.is_some());
        assert!(config.system_prompt.unwrap().contains("sub-agent"));

        match prev {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn build_helve_config_no_skills_dir_no_prompt() {
        let _lock = crate::env_test_lock().lock().unwrap();
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev_dir = std::env::current_dir().ok();
        let _ = std::env::set_current_dir(dir.path());
        let prev_loom = std::env::var("LOOM_HOME").ok();
        std::env::set_var("LOOM_HOME", dir.path());

        let opts = RunOptions {
            message: crate::message::UserContent::Text("hello".to_string()),
            agent: Some("dev".to_string()),
            ..default_opts()
        };
        let (helve, config, resolved_agent) = build_helve_config(&opts);
        assert!(helve.role_setting.is_some());
        assert!(config.skill_registry.is_some());
        let ra = resolved_agent.expect("should resolve dev agent");
        assert_eq!(ra.name, "dev");
        assert_eq!(ra.source, ProfileSource::BuiltIn);

        if let Some(d) = prev_dir {
            let _ = std::env::set_current_dir(d);
        }
        match prev_loom {
            Some(v) => std::env::set_var("LOOM_HOME", v),
            None => std::env::remove_var("LOOM_HOME"),
        }
    }

    #[test]
    fn constants_match() {
        assert_eq!(DEFAULT_WORKING_FOLDER, ".");
        assert_eq!(AGENTS_MD_FILE, "AGENTS.md");
    }

    #[test]
    fn apply_model_provider_resolution_with_provider_model_format() {
        let mut opts = default_opts();
        opts.model = Some("openai/gpt-4o".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        assert_eq!(opts.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn apply_model_provider_resolution_with_bare_model() {
        let mut opts = default_opts();
        opts.model = Some("gpt-4o".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        assert_eq!(opts.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn apply_model_provider_resolution_with_provider_override() {
        let mut opts = default_opts();
        opts.model = Some("anthropic/claude-3-opus".to_string());
        opts.provider = Some("openai".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        assert_eq!(opts.model.as_deref(), Some("claude-3-opus"));
        // Provider should be overridden by --provider flag
        assert_eq!(opts.provider.as_deref(), Some("openai"));
    }

    #[test]
    fn apply_model_provider_resolution_with_provider_only() {
        let mut opts = default_opts();
        opts.provider = Some("openai".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        assert_eq!(opts.provider.as_deref(), Some("openai"));
    }

    #[test]
    fn apply_model_provider_resolution_empty() {
        let mut opts = default_opts();
        
        apply_model_provider_resolution(&mut opts);
        
        assert!(opts.model.is_none());
        assert!(opts.provider.is_none());
    }

    #[test]
    fn apply_model_provider_resolution_multiple_slashes() {
        let mut opts = default_opts();
        opts.model = Some("provider/sub/model".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        // Should split on first slash only
        assert_eq!(opts.model.as_deref(), Some("sub/model"));
    }

    #[test]
    fn apply_model_provider_resolution_empty_model() {
        let mut opts = default_opts();
        opts.model = Some("".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        // Should not modify options when model is empty
        assert!(opts.model.is_none());
        assert!(opts.provider.is_none());
    }

    #[test]
    fn apply_model_provider_resolution_whitespace_model() {
        let mut opts = default_opts();
        opts.model = Some("   ".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        // Should not modify options when model is whitespace only
        assert!(opts.model.is_none());
        assert!(opts.provider.is_none());
    }

    #[test]
    fn apply_model_provider_resolution_empty_provider_in_format() {
        let mut opts = default_opts();
        opts.model = Some("/gpt-4o".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        // Should not modify options when provider part is empty
        assert!(opts.model.is_none());
        assert!(opts.provider.is_none());
    }

    #[test]
    fn apply_model_provider_resolution_empty_model_in_format() {
        let mut opts = default_opts();
        opts.model = Some("openai/".to_string());
        
        apply_model_provider_resolution(&mut opts);
        
        // Should not modify options when model part is empty
        assert!(opts.model.is_none());
        assert!(opts.provider.is_none());
    }
}
