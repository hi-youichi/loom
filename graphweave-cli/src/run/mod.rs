//! Run orchestration for agent patterns (ReAct, ToT, GoT, DUP).
//!
//! Builds [`HelveConfig`](graphweave::HelveConfig) from CLI options, merges with
//! [`ReactBuildConfig::from_env`](graphweave::ReactBuildConfig::from_env) via
//! [`to_react_build_config`](graphweave::to_react_build_config), then invokes the
//! corresponding runner for each pattern.

mod agent;
mod builder;
mod display;

pub(crate) use display::{
    format_dup_state_display, format_got_state_display, format_react_state_display,
    format_tot_state_display, truncate_display,
};
pub use agent::{run_agent, AnyRunner, RunCmd};

use graphweave::{
    build_react_run_context, to_react_build_config, AgentError, BuildRunnerError, HelveConfig,
    ReactBuildConfig,
};
use std::path::PathBuf;
use thiserror::Error;

/// Default working folder when `-w` / `--working-folder` is not passed.
/// File tools (list_dir, read, write_file, etc.) use this directory.
pub(crate) const DEFAULT_WORKING_FOLDER: &str = "/tmp";

/// Options for running the Helve agent from the CLI.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// User message to send to the agent.
    pub message: String,
    /// Working directory (e.g. for tools that need cwd). Default: /tmp when not set.
    pub working_folder: Option<PathBuf>,
    /// Thread ID for checkpointer (conversation continuity).
    pub thread_id: Option<String>,
    /// Enable verbose (node enter/exit) logging.
    pub verbose: bool,
    /// When true, GoT uses AGoT mode (adaptive expansion). Applies only to `got` subcommand.
    pub got_adaptive: bool,
    /// Max length for User/Assistant message content when printing state to stderr.
    pub display_max_len: usize,
}

/// Error type for run operations.
#[derive(Debug, Error)]
pub enum RunError {
    #[error("build runner: {0}")]
    Build(#[from] BuildRunnerError),
    #[error("run: {0}")]
    Run(#[from] graphweave::RunError),
    #[error("dup run: {0}")]
    DupRun(#[from] graphweave::DupRunError),
    #[error("tot run: {0}")]
    TotRun(#[from] graphweave::TotRunError),
    #[error("got run: {0}")]
    GotRun(#[from] graphweave::GotRunError),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
}

/// Reads SOUL.md from current directory and optionally from working_folder, then merges.
/// Order: current dir first, then working_folder (when set and different from cwd). Returns `None`
/// when both are missing or empty.
pub(crate) fn load_soul_md(working_folder: Option<&PathBuf>) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let cwd_canon = cwd.canonicalize().unwrap_or(cwd.clone());
    let cwd_soul = std::fs::read_to_string(cwd.join("SOUL.md"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string());
    let work_soul = working_folder
        .filter(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()) != cwd_canon)
        .and_then(|p| std::fs::read_to_string(p.join("SOUL.md")).ok())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string());
    match (cwd_soul, work_soul) {
        (Some(c), Some(w)) => Some(format!("{}\n\n{}", c, w)),
        (Some(c), None) => Some(c),
        (None, Some(w)) => Some(w),
        (None, None) => None,
    }
}

/// Builds HelveConfig and ReactBuildConfig from RunOptions.
/// Used by all agent pattern runners. Returns (helve, config) for optional SOUL.md logging.
pub(crate) fn build_helve_config(opts: &RunOptions) -> (HelveConfig, ReactBuildConfig) {
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
        role_setting: load_soul_md(Some(&working_folder)),
        system_prompt_override: None,
    };
    let config = to_react_build_config(&helve, base);
    (helve, config)
}

/// Builds the appropriate runner from CLI options and command.
/// Dispatch lives in `run::builder::build_runner`; add new agents by adding a branch there.
pub(crate) async fn build_runner_from_cli(
    _helve: &HelveConfig, // reserved for SOUL/role and future Helve-specific overrides
    config: &mut ReactBuildConfig,
    opts: &RunOptions,
    cmd: &agent::RunCmd,
) -> Result<agent::AnyRunner, RunError> {
    if let agent::RunCmd::Got { got_adaptive } = cmd {
        config.got_config.adaptive = *got_adaptive;
    }
    builder::build_runner(config, opts, cmd).await
}

/// Builds run context, lists tools from the tool source, and prints them to stderr.
/// Called at startup so the user sees which tools are loaded.
pub(crate) async fn print_loaded_tools(config: &ReactBuildConfig) -> Result<(), RunError> {
    let ctx = build_react_run_context(config)
        .await
        .map_err(|e| RunError::Build(BuildRunnerError::Context(e)))?;
    let tools = ctx.tool_source.list_tools().await.map_err(|e| {
        RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
            e.to_string(),
        )))
    })?;
    let names: Vec<&str> = tools.iter().map(|s| s.name.as_str()).collect();
    eprintln!("loaded tools: {}", names.join(", "));
    Ok(())
}
