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
    #[arg(short, long, default_value = "false")]
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

    /// Run in an isolated git worktree. Creates a temporary worktree, executes there, and
    /// cleans up if no changes were made. Preserves the worktree branch if changes exist.
    #[arg(long = "worktree")]
    pub(crate) worktree: bool,

    /// Debug LLM: print full system prompt and messages to stderr before sending to LLM
    #[arg(long)]
    pub(crate) debug_llm: bool,

    /// Log level (tracing EnvFilter syntax). Overrides RUST_LOG when set; default RUST_LOG or info.
    #[arg(long, global = true, value_name = "LEVEL")]
    pub(crate) log_level: Option<String>,

    /// Log file path. Overrides LOG_FILE when set; when neither is set, logs are dropped.
    #[arg(long, global = true, value_name = "PATH")]
    pub(crate) log_file: Option<PathBuf>,

    /// Log rotation strategy: none, daily, hourly, minutely (requires --log-file)
    #[arg(long, global = true, default_value = "daily", value_name = "STRATEGY")]
    pub(crate) log_rotate: String,

    /// Log output format: text (default) or json
    #[arg(long, global = true, default_value = "text", value_name = "FORMAT")]
    pub(crate) log_format: String,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum Command {
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
    /// Manage agent profiles (list, export)
    Agent(AgentArgs),
    /// Run autonomous goal loop with an external coding tool
    Goal(GoalArgs),
    /// Manage skills (list, show, create, edit, delete)
Skills(SkillsArgs),
    /// Sync, show, and repair skill usage tracking (.usage.json)
    SkillUsage(SkillUsageArgs),
    /// Run and manage skill evolution
    Evolve,
    /// Manage skill lifecycle (stale detection, archiving)
    Curator(CuratorCmdArgs),
    /// View and edit agent memory (user preferences, project facts)
    Memory(MemoryCmdArgs),
    /// Review session or files to extract skills and memory updates
    ReviewSkill(ReviewSkillArgs),
    /// Review sessions to extract skills and memory updates
    Review(ReviewArgs),
    /// Create and manage company tasks (AI Company mode)
    Task(TaskArgs),
    /// Run interactive TUI with persistent input bar and status bar
    Tui(TuiArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ReviewSkillArgs {
    /// Input file to review (omit to read from stdin)
    #[arg(long)]
    pub(crate) input: Option<PathBuf>,
    /// Model to use for review
    #[arg(long)]
    pub(crate) model: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ReviewArgs {
    #[command(subcommand)]
    pub(crate) command: ReviewCommand,

    /// Model to use for review (overrides config/env default)
    #[arg(long, value_name = "MODEL")]
    pub(crate) model: Option<String>,

    /// Verbose output
    #[arg(long)]
    pub(crate) verbose: bool,

    /// Dry run: show what would be reviewed without calling LLM
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Only extract memory updates (skip skills)
    #[arg(long)]
    pub(crate) memory_only: bool,

    /// Only extract skill suggestions (skip memory)
    #[arg(long)]
    pub(crate) skills_only: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum ReviewCommand {
    /// Review a single session by session ID
    Session {
        /// Session ID to review
        session_id: String,
        /// Trigger source for review history (manual, background, batch)
        #[arg(long, default_value = "manual")]
        trigger: String,
    },
    /// Batch review multiple sessions
    Sessions {
        /// Review sessions from the last N days (e.g. "7d", "30d")
        #[arg(long, value_name = "DURATION")]
        recent: Option<String>,
        /// Review all unreviewed sessions
        #[arg(long)]
        all_unreviewed: bool,
        /// Search sessions by keyword and review matches
        #[arg(long, value_name = "QUERY")]
        query: Option<String>,
    },
    /// Show review history
    History {
        /// Filter by trigger type: manual, auto, batch
        #[arg(long)]
        trigger: Option<String>,
        /// Show last N records (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Show review result for a specific session
    Show {
        /// Session ID
        session_id: String,
    },
    /// List sessions that have not been reviewed yet
    Pending {
        /// Maximum sessions to list (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
    },
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

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct AgentArgs {
    #[command(subcommand)]
    pub(crate) command: AgentCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum AgentCommand {
    /// List available agent profiles
    List,
    /// Export agent profile to third-party tool format
    Export(ExportArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ExportArgs {
    /// Export format: claude-code, codex, cursor
    #[arg(value_name = "FORMAT")]
    pub(crate) format: String,

    /// Agent profile name (default: all project agents)
    #[arg(value_name = "AGENT")]
    pub(crate) agent: Option<String>,

    /// Output directory (default: current directory)
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub(crate) output: PathBuf,

    /// Dry run: print to stdout instead of writing files
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct GoalArgs {
    /// Goal description (objective to achieve)
    #[arg(value_name = "DESCRIPTION")]
    pub(crate) description: Option<String>,

    /// External coding tool to use: codex, claude, cursor, or a custom command
    #[arg(short, long, value_name = "TOOL", default_value = "loom")]
    pub(crate) tool: String,

    /// Resume a paused goal by task ID (prefix)
    #[arg(long, value_name = "ID")]
    pub(crate) resume: Option<String>,

    /// Use a specific task ID instead of generating one
    #[arg(long, value_name = "ID")]
    pub(crate) id: Option<String>,

    /// Print verbose iteration info to stderr
    #[arg(long)]
    pub(crate) verbose: bool,

    /// Override LLM model for goal turns (e.g. "gpt-4o", "zhipuai-coding-plan/glm-5.1")
    #[arg(short('M'), long, value_name = "MODEL")]
    pub(crate) model: Option<String>,

    /// Hard cap on total tokens consumed across all iterations
    #[arg(long, value_name = "TOKENS")]
    pub(crate) token_budget: Option<u32>,

    /// Shell command to verify objective after each iteration (e.g. "cargo test")
    #[arg(long, value_name = "CMD")]
    pub(crate) verify: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct SkillsArgs {
    #[command(subcommand)]
    pub(crate) command: SkillsCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum SkillsCommand {
    /// List all skills
    List,
    /// Show skill details
    Show { name: String },
    /// Create a new skill
    Create {
        name: String,
        #[arg(long, value_name = "DESC")]
        description: Option<String>,
        #[arg(long = "trigger", value_name = "KW")]
        triggers: Vec<String>,
    },
    /// Edit an existing skill (opens $EDITOR)
    Edit { name: String },
    /// Delete a skill
    Delete { name: String },
}

/// Arguments for the `skill-usage` subcommand.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct SkillUsageArgs {
    #[command(subcommand)]
    pub sub: SkillUsageCommand,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum SkillUsageCommand {
    /// Scan skills directory and sync .usage.json
    Sync {
        /// Skills root directory (default: ~/.loom/data/skills)
        #[arg(long, value_name = "PATH")]
        path: Option<PathBuf>,
        /// Preview changes without writing files
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Filter by source: auto, curated, evolved, or all (default)
        #[arg(long, value_name = "SOURCE", default_value = "all")]
        source: String,
    },
    /// Show current .usage.json content
    Show {
        /// Skill name to show (omit to show all)
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Repair a corrupted .usage.json
    Repair {
        /// Skills root directory (default: ~/.loom/data/skills)
        #[arg(long, value_name = "PATH")]
        path: Option<PathBuf>,
    },
}

#[derive(clap::Subcommand, Debug, Clone)]
pub(crate) enum CuratorCommand {
    /// Run curator review (automatic state transitions + LLM pass)
    Run {
        /// Force LLM pass even if interval gating would skip it
        #[arg(long)]
        force: bool,
    },
    /// Show curator status and statistics
    Status,
    /// Bulk archive old skills
    Prune {
        /// Archive skills idle for at least N days
        #[arg(long, default_value = "90")]
        days: u32,
    },
    /// Pause curator (skip next scheduled runs)
    Pause,
    /// Resume curator (enable scheduled runs)
    Resume,
    /// Pin a skill so the curator never archives or consolidates it
    Pin {
        /// Name of the skill to pin
        #[arg(value_name = "SKILL")]
        skill: String,
    },
    /// Remove a pin from a skill
    Unpin {
        /// Name of the skill to unpin
        #[arg(value_name = "SKILL")]
        skill: String,
    },
    /// Restore an archived skill back to Active
    Restore {
        /// Name of the skill to restore
        #[arg(value_name = "SKILL")]
        skill: String,
    },
    /// Manually archive a single skill (Lifecycle → Archived)
    Archive {
        /// Name of the skill to archive
        #[arg(value_name = "SKILL")]
        skill: String,
    },
    /// Create a backup snapshot of the entire skill library
    Backup {
        /// Optional description for the snapshot
        #[arg(long)]
        description: Option<String>,
    },
    /// Roll back the skill library to a previous snapshot
    Rollback {
        /// Snapshot filename (e.g. curator-2025-08-19T12-34-56.tar.gz)
        #[arg(value_name = "SNAPSHOT")]
        snapshot: String,
    },
    /// List available backup snapshots
    Snapshots,
    /// Backfill triggers for skills that have an empty trigger list
    BackfillTriggers {
        /// Only process a specific skill by name
        #[arg(long, value_name = "SKILL")]
        skill: Option<String>,
        /// Number of skills per LLM call (default: 10)
        #[arg(long, default_value = "10", value_name = "N")]
        batch_size: usize,
    },
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct CuratorCmdArgs {
    #[command(subcommand)]
    pub(crate) command: CuratorCommand,
    /// Dry run: report but don't modify
    #[arg(long)]
    pub(crate) dry_run: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct MemoryCmdArgs {
    #[command(subcommand)]
    pub(crate) command: MemoryCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum MemoryCommand {
    /// Show all memory files
    Show,
    /// Edit a memory file (opens $EDITOR)
    Edit {
        #[arg(value_name = "FILE")]
        file: String,
    },
    /// Search memory for a keyword
    Search {
        query: String,
    },
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct TaskArgs {
    #[command(subcommand)]
    pub(crate) command: TaskCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum TaskCommand {
    /// Create a new task and start CEO agent to process it
    New {
        /// Task description (what you want done)
        description: Vec<String>,
        /// Override agent (default: ceo)
        #[arg(short, long, value_name = "AGENT", default_value = "ceo")]
        agent: String,
        /// Override LLM model
        #[arg(short('M'), long, value_name = "MODEL")]
        model: Option<String>,
    },
    /// List all tasks
    List {
        /// Filter by status
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        /// Filter by assignee
        #[arg(long, value_name = "ASSIGNEE")]
        assignee: Option<String>,
    },
    /// Show task details
    Show {
        /// Task ID (or prefix)
        id: String,
    },
    /// Continue a task in interactive mode (resume with CEO agent)
    Continue {
        /// Task ID (or prefix)
        id: String,
        /// Override agent (default: ceo)
        #[arg(short, long, value_name = "AGENT", default_value = "ceo")]
        agent: String,
    },
}

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct TuiArgs {
    /// Override LLM model for TUI sessions
    #[arg(short('M'), long, value_name = "MODEL")]
    pub(crate) model: Option<String>,

    /// Override LLM provider name
    #[arg(long, value_name = "PROVIDER")]
    pub(crate) provider: Option<String>,

    /// Named agent profile
    #[arg(short('P'), long, value_name = "NAME")]
    pub(crate) agent: Option<String>,

    /// Session ID for conversation continuity
    #[arg(long, value_name = "ID")]
    pub(crate) session_id: Option<String>,

    /// Working folder
    #[arg(short, long, value_name = "DIR")]
    pub(crate) working_folder: Option<PathBuf>,

    /// Path to MCP config JSON
    #[arg(long, value_name = "PATH")]
    pub(crate) mcp_config: Option<PathBuf>,
}
