//! Build a `ReactBuildConfig` from an agent profile.
//!
//! This function is in `loom-react-config` (not `loom`) to avoid circular
//! dependencies: `loom` → `loom-agent` → `loom-agent-patterns`, and
//! `loom-agent-patterns` needs this function.

use std::path::PathBuf;
use std::sync::Arc;

use crate::profile::AgentProfile;
use crate::agent::ReactBuildConfig;
use tool_core::BuiltinToolFilter;
use skill::discovery::SkillRegistry;


const AGENTS_MD_FILE: &str = "AGENTS.md";

/// Build a sub-agent's `ReactBuildConfig` by overlaying profile settings on
/// top of a parent config.
///
/// This is the core logic for `agent` tool sub-agent configuration:
/// it inherits most fields from `parent_config`, then applies profile-specific
/// overrides for model, tools, skills, and working folder.
///
/// The `system_prompt` field is NOT assembled here — callers should handle
/// prompt assembly themselves via `crate::prompt_assembly`.
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
        .unwrap_or_else(|| PathBuf::from("."));

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

    // System prompt: extract role content from profile
    if let Some(ref role) = profile.role {
        if let Some(ref content) = role.content {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                // Load AGENTS.md from working folder
                let agents_md = load_agents_md(config.working_folder.as_ref());
                let mut parts = vec![];
                if let Some(ref base) = config.system_prompt {
                    if !base.is_empty() {
                        parts.push(base.clone());
                    }
                }
                parts.push(trimmed.to_string());
                if let Some(amd) = agents_md {
                    parts.push(amd);
                }
                config.system_prompt = Some(parts.join("\n\n"));
            }
        }
    } else {
        // No role in profile, still check for AGENTS.md
        let agents_md = load_agents_md(config.working_folder.as_ref());
        if let Some(md) = agents_md {
            let mut parts = vec![];
            if let Some(ref base) = config.system_prompt {
                if !base.is_empty() {
                    parts.push(base.clone());
                }
            }
            parts.push(md);
            config.system_prompt = Some(parts.join("\n\n"));
        }
    }

    // Skill registry for sub-agent
    let extra_dirs: Vec<PathBuf> = profile
        .skills
        .as_ref()
        .and_then(|s| s.dirs.as_ref())
        .map(|dirs| dirs.iter().map(PathBuf::from).collect())
        .unwrap_or_default();
    let mut registry = SkillRegistry::discover(&working_folder, &extra_dirs)
        .unwrap_or_else(|e| {
            tracing::warn!("skill discovery failed: {e}");
            SkillRegistry::empty()
        });
    if let Some(ref src) = profile.source_dir {
        if let Err(e) = registry.add_agent_skills(&src.join("skills")) {
            tracing::warn!("agent skills scan failed: {e}");
        }
    }
    if let Some(ref sc) = profile.skills {
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
    config.skill_registry = Some(Arc::new(registry));

    config.max_sub_agent_depth = profile
        .behavior
        .as_ref()
        .and_then(|b| b.max_sub_agent_depth)
        .or(parent_config.max_sub_agent_depth);

    // Builtin tool filter from profile
    if let Some(ref tools) = profile.tools {
        if let Some(ref builtin) = tools.builtin {
            let filter = BuiltinToolFilter {
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

/// Load AGENTS.md from the given working folder (and current directory).
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
