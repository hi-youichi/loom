//! Slash command types: parsed command enum and execution result.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ResetContext,
    Compact { instructions: Option<String> },
    Summarize,
    Models { query: Option<String> },
    ModelsUse { model_id: String },
    Goal { description: String },
    /// Trigger a background review of the current session to extract skills and memory.
    ReviewSkill { scope: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    Reply(String),
    PassThrough,
}
