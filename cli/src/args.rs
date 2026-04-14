//! Clap definitions for the `loom` binary.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::session::SessionArgs;

/// Config directory: ~/.loom (or $LOOM_HOME). config.toml [env] is applied as env vars; project .env overrides.
pub(crate) const CONFIG_DIR_HELP: &str = "\nConfiguration:\n  Config directory: ~/.loom (override with $LOOM_HOME).\n  File: config.toml with [env] table; values are applied as environment variables.\n  Project .env in working directory overrides config.toml.";

#[derive(Parser, Debug)]
#[command(name = "loom")]
#[command(about = "Loom — run ReAct or DUP agent from CLI", after_help = CONFIG_DIR_HELP)]
pub(crate) struct Args {
    #[command(subcommand)]
    pub(crate) cmd: Option<Command>,

    /// User message (or pass as first positional argument)
    #[arg(short, long, value_name = "TEXT")]
    pub(crate) message: Option<String>,

    /// Positional args: user message when -m/--message is not used
    #[arg(trailing_var_arg = true)]
    pub(crate) rest: Vec<String>,

    /// Working folder (for file tools); default: current directory when not set
    #[arg(short, long, value_name = "DIR")]
    pub(crate) working_folder: Option<PathBuf>,

    /// Override LLM model for this run. Supports bare name ("gpt-4o") or "provider/model" format
    /// (e.g. "zhipuai-coding-plan/glm-5.1") to auto-select provider from [[providers]] in config.toml.
    #[arg(short('M'), long, value_name = "MODEL")]
    pub(crate) model: Option<String>,

    /// Override LLM provider name from [[providers]] in config.toml (e.g. "openai", "zhipuai-coding-plan").
    /// When set, takes precedence over the provider/ prefix in --model.
    #[arg(long, value_name = "PROVIDER")]
    pub(crate) provider: Option<String>,

    /// Named agent profile (e.g. coding). Loaded from .loom/agents/<NAME> or ~/.loom/agents/<NAME>.
    #[arg(short('P'), long, value_name = "NAME")]
    pub(crate) agent: Option<String>,

    /// Session ID for conversation continuity (checkpointer)
    #[arg(long, value_name = "ID")]
    pub(crate) session_id: Option<String>,

    /// Print State info to stderr (node enter/exit, state after each step, flow)
    #[arg(short, long, default_value = "true")]
    pub(crate) verbose: bool,

    /// Interactive REPL: after output, prompt for input and continue conversation
    #[arg(short, long)]
    pub(crate) interactive: bool,

    /// Output all data as JSON (stream events + reply for agent run; JSON array for tool list; JSON for tool show)
    #[arg(long)]
    pub(crate) json: bool,

    /// When using --json, write output to this file instead of stdout
    #[arg(long, value_name = "PATH")]
    pub(crate) file: Option<PathBuf>,

    /// When using --json, pretty-print (multi-line). Default: compact, one line per event
    #[arg(long)]
    pub(crate) pretty: bool,

    /// Print a timestamp to stderr before each reply (local time, e.g. 2025-03-15 10:30:00)
    #[arg(long)]
    pub(crate) timestamp: bool,

    /// Path to MCP config JSON (overrides LOOM_MCP_CONFIG_PATH and default .loom/mcp.json discovery)
    #[arg(long, value_name = "PATH")]
    pub(crate) mcp_config: Option<PathBuf>,

    /// Dry run: LLM runs but tools are not executed (placeholder result returned)
    #[arg(long)]
    pub(crate) dry: bool,

    /// Log level (tracing EnvFilter syntax). Overrides RUST_LOG when set; default RUST_LOG or info.
    #[arg(long, global = true, value_name = "LEVEL")]
    pub(crate) log_level: Option<String>,

    /// Log file path. Overrides LOG_FILE when set; when neither is set, logs are dropped.
    #[arg(long, global = true, value_name = "PATH")]
    pub(crate) log_file: Option<PathBuf>,

    /// Log rotation strategy: none, daily, hourly, minutely (requires --log-file)
    #[arg(long, global = true, default_value = "daily", value_name = "STRATEGY")]
    pub(crate) log_rotate: String,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum Command {
    /// Run WebSocket server (ws://127.0.0.1:8080)
    Serve(ServeArgs),
    /// Run ReAct graph (think → act → observe)
    React,
    /// Run DUP graph (understand → plan → act → observe)
    Dup,
    /// Run ToT graph (think_expand → think_evaluate → act → observe)
    Tot,
    /// Run GoT graph (plan_graph → execute_graph)
    Got(GotArgs),
    /// List or show tool definitions (same tools as used by react/dup/tot/got)
    Tool(ToolArgs),
    /// Manage conversation sessions (list, show, delete)
    Session(SessionArgs),
    /// List available models from configured providers
    Models(ModelsArgs),
    /// Manage MCP servers (list, show, add, edit, delete, enable, disable)
    Mcp(McpArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ToolArgs {
    #[command(subcommand)]
    pub(crate) sub: ToolCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum ToolCommand {
    /// List all loaded tools (name and description)
    List,
    /// Show full definition of one tool (name, description, input_schema)
    Show(ShowToolArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ShowToolArgs {
    /// Tool name (e.g. read, web_fetcher)
    pub(crate) name: String,
    /// Output format: yaml (default) or json
    #[arg(long, value_name = "FORMAT", default_value = "yaml")]
    pub(crate) output: String,
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ModelsArgs {
    #[command(subcommand)]
    pub(crate) sub: ModelsCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum ModelsCommand {
    /// List available models from all configured providers
    List,
    /// List models from a specific provider
    Show(ShowModelsArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ShowModelsArgs {
    /// Provider name (e.g., openai, bigmodel)
    pub(crate) name: String,
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ServeArgs {
    /// WebSocket listen address (default 127.0.0.1:8080)
    #[arg(long, value_name = "ADDR")]
    pub(crate) addr: Option<String>,
}

/// Arguments for the `got` subcommand.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct GotArgs {
    /// Enable AGoT adaptive mode (expand complex nodes).
    #[arg(long)]
    pub(crate) got_adaptive: bool,
}

/// Arguments for the `mcp` subcommand.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct McpArgs {
    #[command(subcommand)]
    pub(crate) command: McpCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum McpCommand {
    /// List all MCP servers
    List,
    /// Show details of a specific MCP server
    Show {
        /// Server name
        name: String,
    },
    /// Add a new MCP server
    Add(AddMcpArgs),
    /// Edit an existing MCP server
    Edit(EditMcpArgs),
    /// Delete an MCP server
    Delete {
        /// Server name to delete
        name: String,
    },
    /// Enable a disabled MCP server
    Enable {
        /// Server name to enable
        name: String,
    },
    /// Disable an enabled MCP server
    Disable {
        /// Server name to disable
        name: String,
    },
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct AddMcpArgs {
    /// Server name
    #[arg(long, value_name = "NAME")]
    pub(crate) name: String,

    /// Command for stdio-based servers (e.g., "npx")
    #[arg(long, value_name = "CMD")]
    pub(crate) command: Option<String>,

    /// Arguments for the command (can be specified multiple times)
    #[arg(long = "arg", value_name = "ARG", allow_hyphen_values = true)]
    pub(crate) args: Vec<String>,

    /// URL for HTTP-based servers
    #[arg(long, value_name = "URL")]
    pub(crate) url: Option<String>,

    /// Environment variables (KEY=VALUE format, can be specified multiple times)
    #[arg(long = "env", value_name = "ENV", allow_hyphen_values = true)]
    pub(crate) env: Vec<String>,

    /// Create server in disabled state
    #[arg(long)]
    pub(crate) disabled: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct EditMcpArgs {
    /// Server name to edit
    #[arg(value_name = "NAME")]
    pub(crate) name: String,

    /// New command for stdio-based servers
    #[arg(long, value_name = "CMD")]
    pub(crate) command: Option<String>,

    /// New arguments for the command (can be specified multiple times)
    #[arg(long = "arg", value_name = "ARG", allow_hyphen_values = true)]
    pub(crate) args: Vec<String>,

    /// New URL for HTTP-based servers
    #[arg(long, value_name = "URL")]
    pub(crate) url: Option<String>,

    /// New environment variables (KEY=VALUE format, can be specified multiple times)
    #[arg(long = "env", value_name = "ENV", allow_hyphen_values = true)]
    pub(crate) env: Vec<String>,

    /// Set disabled state (true/false)
    #[arg(long, value_name = "BOOL")]
    pub(crate) disabled: Option<bool>,
}
