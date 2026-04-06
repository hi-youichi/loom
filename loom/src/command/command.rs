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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    Reply(String),
    PassThrough,
}
