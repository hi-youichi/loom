//! Run orchestration for agent patterns (ReAct, ToT, GoT, DUP).
//!
//! Builds HelveConfig and ReactBuildConfig, then invokes the corresponding runner.
//! Used by both cli (local) and loom serve (remote).

mod agent;
mod profile;

pub use agent::{
    run_agent, run_agent_with_options, run_agent_with_llm_override, AnyRunner, AnyStreamEvent,
    RunCmd, RunError, RunOptions,
};

use crate::{to_react_build_config, HelveConfig, ReactBuildConfig};
use std::path::PathBuf;

pub use profile::{load_profile_from_options, AgentProfile, ProfileError};

/// Default working folder when not set (current directory).
pub const DEFAULT_WORKING_FOLDER: &str = ".";

const AGENTS_MD_FILE: &str = "AGENTS.md";
const INSTRUCTIONS_MD_FILE: &str = "instructions.md";
const SOUL_MD_FILE: &str = "SOUL.md"; // legacy; prefer instructions.md

/// Default instructions (agent persona) embedded at compile time. Same as built-in dev agent (loom/agents/dev/instructions.md). Used when no instructions.md (or SOUL.md) is found on disk.
const DEFAULT_SOUL: &str = include_str!("../../agents/dev/instructions.md");

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

/// Reads instructions.md (or legacy SOUL.md) from current directory and optionally from working_folder.
pub fn load_soul_md(working_folder: Option<&PathBuf>) -> Option<String> {
    let read_instructions = |p: &std::path::Path| {
        std::fs::read_to_string(p.join(INSTRUCTIONS_MD_FILE))
            .or_else(|_| std::fs::read_to_string(p.join(SOUL_MD_FILE)))
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
    };
    let cwd = std::env::current_dir().ok()?;
    let cwd_canon = cwd.canonicalize().unwrap_or(cwd.clone());
    let cwd_soul = read_instructions(&cwd);
    let work_soul = working_folder
        .filter(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()) != cwd_canon)
        .and_then(|p| read_instructions(p));
    match (cwd_soul, work_soul) {
        (Some(c), Some(w)) => Some(format!("{}\n\n{}", c, w)),
        (Some(c), None) => Some(c),
        (None, Some(w)) => Some(w),
        (None, None) => None,
    }
}

/// Resolves role_setting: profile role > --role file > instructions.md (or SOUL.md) > built-in default.
fn resolve_role_setting(
    opts: &RunOptions,
    working_folder: &PathBuf,
    profile_role: Option<String>,
) -> Option<String> {
    if let Some(s) = profile_role {
        if !s.trim().is_empty() {
            return Some(s);
        }
    }
    if let Some(ref path) = opts.role_file {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                let t = s.trim().to_string();
                if !t.is_empty() {
                    return Some(t);
                }
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "role file unreadable, falling back to instructions/default"
                );
            }
        }
    }
    load_soul_md(Some(working_folder)).or_else(|| Some(DEFAULT_SOUL.trim().to_string()))
}

/// Builds HelveConfig and ReactBuildConfig from RunOptions.
pub fn build_helve_config(opts: &RunOptions) -> (HelveConfig, ReactBuildConfig) {
    let profile = load_profile_from_options(opts);
    let mut effective_opts = opts.clone();
    if let Some(ref p) = profile {
        apply_profile_to_run_options(p, &mut effective_opts);
    }

    let mut base = ReactBuildConfig::from_env();
    if let Some(ref m) = effective_opts.model {
        base.model = Some(m.clone());
    }
    let working_folder = effective_opts
        .working_folder
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORKING_FOLDER));

    let profile_role = profile
        .as_ref()
        .and_then(|p| p.role.as_ref().and_then(|r| r.content.clone()));

    // MCP config: CLI > profile > LOOM_MCP_CONFIG_PATH > discover
    let override_path = effective_opts
        .mcp_config_path
        .clone()
        .or_else(|| std::env::var("LOOM_MCP_CONFIG_PATH").ok().map(PathBuf::from));
    if let Some(path) = env_config::discover_mcp_config_path(
        override_path.as_deref(),
        Some(&working_folder),
    ) {
        match env_config::load_mcp_config_from_path(&path) {
            Ok(servers) => base.mcp_servers = Some(servers),
            Err(e) => tracing::warn!(path = %path.display(), "failed to load mcp config: {}", e),
        }
    }

    let helve = HelveConfig {
        working_folder: Some(working_folder.clone()),
        thread_id: effective_opts.thread_id.clone(),
        user_id: base.user_id.clone(),
        approval_policy: None,
        role_setting: resolve_role_setting(opts, &working_folder, profile_role),
        agents_md: load_agents_md(Some(&working_folder)),
        system_prompt_override: None,
    };
    let config = to_react_build_config(&helve, base);
    (helve, config)
}

fn apply_profile_to_run_options(profile: &AgentProfile, opts: &mut RunOptions) {
    if let Some(ref role) = profile.role {
        if opts.role_file.is_none() && role.content.is_some() {
            // role_setting will be taken from profile in build_helve_config
        }
    }
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
