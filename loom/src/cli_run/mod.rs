//! Run orchestration for agent patterns (ReAct, ToT, GoT, DUP).
//!
//! Builds HelveConfig and ReactBuildConfig, then invokes the corresponding runner.
//! Used by both cli (local) and loom serve (remote).

mod agent;

pub use agent::{run_agent, AnyRunner, AnyStreamEvent, RunCmd, RunError, RunOptions};

use crate::{to_react_build_config, HelveConfig, ReactBuildConfig};
use std::path::PathBuf;

/// Default working folder when not set.
pub const DEFAULT_WORKING_FOLDER: &str = "/tmp";

const AGENTS_MD_FILE: &str = "AGENTS.md";
const SOUL_MD_FILE: &str = "SOUL.md";

/// Default SOUL (agent persona) embedded at compile time. Used when no SOUL.md is found on disk.
const DEFAULT_SOUL: &str = include_str!("../../prompts/SOUL.md");

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

/// Reads SOUL.md from current directory and optionally from working_folder.
pub fn load_soul_md(working_folder: Option<&PathBuf>) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let cwd_canon = cwd.canonicalize().unwrap_or(cwd.clone());
    let cwd_soul = std::fs::read_to_string(cwd.join(SOUL_MD_FILE))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string());
    let work_soul = working_folder
        .filter(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()) != cwd_canon)
        .and_then(|p| std::fs::read_to_string(p.join(SOUL_MD_FILE)).ok())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string());
    match (cwd_soul, work_soul) {
        (Some(c), Some(w)) => Some(format!("{}\n\n{}", c, w)),
        (Some(c), None) => Some(c),
        (None, Some(w)) => Some(w),
        (None, None) => None,
    }
}

/// Resolves role_setting: --role file if set and readable, else SOUL.md then built-in default.
fn resolve_role_setting(opts: &RunOptions, working_folder: &PathBuf) -> Option<String> {
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
                    "role file unreadable, falling back to SOUL.md/default"
                );
            }
        }
    }
    load_soul_md(Some(working_folder)).or_else(|| Some(DEFAULT_SOUL.trim().to_string()))
}

/// Builds HelveConfig and ReactBuildConfig from RunOptions.
pub fn build_helve_config(opts: &RunOptions) -> (HelveConfig, ReactBuildConfig) {
    let base = ReactBuildConfig::from_env();
    let working_folder = opts
        .working_folder
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORKING_FOLDER));
    let helve = HelveConfig {
        working_folder: Some(working_folder.clone()),
        thread_id: opts.thread_id.clone(),
        user_id: base.user_id.clone(),
        approval_policy: None,
        role_setting: resolve_role_setting(opts, &working_folder),
        agents_md: load_agents_md(Some(&working_folder)),
        system_prompt_override: None,
    };
    let config = to_react_build_config(&helve, base);
    (helve, config)
}
