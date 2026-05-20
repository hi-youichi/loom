use crate::llm::LlmUsage;

pub struct TurnResult {
    pub reply: String,
    pub reasoning_content: Option<String>,
    pub tool_calls_summary: Vec<ToolCallSummary>,
    pub usage: Option<LlmUsage>,
}

pub struct ToolCallSummary {
    pub tool_name: String,
    pub result_preview: String,
}

pub enum ToolError {
    ExecutionFailed(String),
    Timeout,
    Aborted,
}

#[derive(Debug)]
pub enum GoalOutcome {
    Achieved,
    Error(String),
}

impl std::fmt::Display for GoalOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoalOutcome::Achieved => write!(f, "Goal achieved"),
            GoalOutcome::Error(e) => write!(f, "Goal error: {}", e),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GoalError {
    #[error("database error: {0}")]
    Db(#[from] Box<dyn std::error::Error>),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("invalid resume state: {0}")]
    Resume(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalMeta {
    pub iteration: u32,
    pub tool: String,
    pub time_used_seconds: i64,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub iteration: u32,
    pub timestamp: String,
}

impl Default for GoalMeta {
    fn default() -> Self {
        Self {
            iteration: 0,
            tool: "loom".to_string(),
            time_used_seconds: 0,
            history: Vec::new(),
        }
    }
}

pub const DEFAULT_MAX_ITERATIONS: u32 = 100;
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;
pub const MAX_HISTORY_ENTRIES: usize = 20;
