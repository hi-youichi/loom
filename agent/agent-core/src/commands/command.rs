//! Slash command types: parsed command enum and execution result.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ResetContext,
    Compact {
        instructions: Option<String>,
    },
    Summarize,
    Models {
        query: Option<String>,
    },
    ModelsUse {
        model_id: String,
    },
    Goal {
        description: String,
    },
    /// Trigger a background review of the current session to extract skills and memory.
    ReviewSkill {
        scope: Option<String>,
    },
    // Priority #18 (Hermes parity, `cli.py`): expand slash command
    // surface to cover the user-facing gap. Each variant maps to a single
    // user intent — the executor in `apps/cli/src/repl.rs::handle_repl_command`
    // dispatches them.
    /// `/help [command]` — list every slash command or print help for one.
    Help {
        command: Option<String>,
    },
    /// `/tools` — list available tools (matches Hermes parity).
    Tools,
    /// `/model <id>` — alias of `/models use <id>` for fast switching.
    Model {
        model_id: Option<String>,
    },
    /// `/resume <title|id>` — switch to a previous session by title prefix or thread id.
    Resume {
        selector: Option<String>,
    },
    /// `/undo` — roll back the most recent assistant turn.
    Undo,
    /// `/retry` — re-run the most recent user prompt.
    Retry,
    /// `/history [n]` — print the last N (default 10) user prompts in this session.
    History {
        count: Option<usize>,
    },
    /// `/exit` — leave the REPL cleanly.
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    Reply(String),
    PassThrough,
}
