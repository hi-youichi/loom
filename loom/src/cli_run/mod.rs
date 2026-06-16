//! Run orchestration for agent patterns (ReAct, ToT, GoT, DUP).
//!
//! This module retains orchestration functions that depend on loom internals.
//! All types live in their own crates — consumers should import directly:
//! - Profile types → `loom_react_config::profile::*`
//! - `load_agents_md` → `loom_react_config::load_agents_md`
//! - `build_config_from_profile` → `loom_react_config::build_config_from_profile`

mod profile;

use loom_react_config::ReactBuildConfig;
use skill::discovery::SkillRegistry;
use loom_prompt::env_context::{EnvContext, ProjectInfo};
use loom_prompt::{SystemPromptInputs, assemble_system_prompt};
use std::path::PathBuf;
use std::sync::Arc;

use loom_cli_types::{
    RunOptions,
    ResolvedAgent, ResolvedModelConfig,
    DEFAULT_WORKING_FOLDER,
};

use loom_react_config::load_agents_md;

// Internal profile helper — not re-exported.
use profile::load_profile_from_options;
use loom_react_config::profile::AgentProfile;

/// Reads memory prompt from LOOM_HOME/data/memory/.
pub fn load_memory_prompt() -> Option<String> {
    let memory_dir = env_config::home::loom_home().join("data").join("memory");
    let store = memory_v2::MemoryStore::new(&memory_dir);
    store.capture_snapshot().ok().filter(|s| !s.is_empty())
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

/// Builds ReactBuildConfig from RunOptions.
/// Returns the config and an optional `ResolvedAgent` describing which agent profile was loaded.
pub fn build_react_config(
    opts: &RunOptions,
) -> (ReactBuildConfig, Option<ResolvedAgent>) {
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
    base.goal_mode = effective_opts.goal_mode;
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
    base.working_folder = Some(working_folder.clone());
    base.thread_id = effective_opts.thread_id.clone().or(base.thread_id.clone());

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
        let mut registry = SkillRegistry::discover(&working_folder, &extra_dirs)
            .unwrap_or_else(|e| {
                tracing::warn!("skill discovery failed: {e}");
                SkillRegistry::empty()
            });
        if let Some(ref p) = profile {
            if let Some(ref src) = p.source_dir {
                if let Err(e) = registry.add_agent_skills(&src.join("skills")) {
                    tracing::warn!("agent skills scan failed: {e}");
                }
            }
            if let Some(ref sc) = p.skills {
                let platform = Some(std::env::consts::OS);
                let platform_disabled: Vec<String> = sc.platform_disabled.as_ref()
                    .and_then(|m| m.get(std::env::consts::OS))
                    .cloned()
                    .unwrap_or_default();
                registry.apply_filters(
                    sc.enabled.as_deref(),
                    sc.disabled.as_deref(),
                    platform,
                    Some(&platform_disabled),
                );
                registry.apply_toolset_filters(None, None);
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

    // Transfer base into config for final assembly
    let mut config = base;

    // Set prompt assembly fields directly on config
    config.role_setting = agent_instructions;
    config.agents_md = load_agents_md(Some(&working_folder));
    config.skills_prompt = skills_prompt;
    config.memory_prompt = load_memory_prompt();
    config.env_context = Some({
        let mut ctx = EnvContext::detect().with_project(
            ProjectInfo::detect(&working_folder),
        );
        if let Some(cid) = effective_opts.chat_id {
            ctx = ctx.with_chat_id(cid);
        }
        ctx
    });

    // Assemble system prompt from config fields
    let env_ctx = config.env_context.as_ref();
    let inputs = SystemPromptInputs {
        full_override: None,
        base_prompt_override: None,
        role_setting: config.role_setting.as_deref(),
        agents_md: config.agents_md.as_deref(),
        skills_prompt: config.skills_prompt.as_deref(),
        memory_prompt: config.memory_prompt.as_deref(),
        env_context: env_ctx,
        working_folder: config.working_folder.as_deref(),
    };
    config.system_prompt = Some(assemble_system_prompt(&inputs));

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

    (config, resolved_agent)
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

    let providers: Vec<loom_tier::model_registry::ProviderConfig> =
        loom_tier::provider::load_provider_configs().unwrap_or_default();

    // 1. Try ModelRegistry first
    tracing::debug!("🔎 Searching ModelRegistry for: {}", model_str);
    if let Some(entry) = loom_tier::model_registry::ModelRegistry::global()
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
