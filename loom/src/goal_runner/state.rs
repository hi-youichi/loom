use crate::llm::LlmUsage;

pub struct TurnResult {
    pub reply: String,
    pub reasoning_content: Option<String>,
    pub tool_calls_summary: Vec<ToolCallSummary>,
    pub usage: Option<LlmUsage>,
    /// Optional work summary for history injection (produced by the tool or post-processed).
    pub work_summary: Option<String>,
}

pub struct ToolCallSummary {
    pub tool_name: String,
    pub result_preview: String,
}

pub enum ToolError {
    ExecutionFailed(String),
    Timeout,
    Aborted,
    /// Transient API error (rate-limit, overload) — retryable with backoff.
    RateLimited(String),
}

impl ToolError {
    /// Returns true if the `ExecutionFailed` message looks like a transient
    /// API error that is worth retrying (rate-limit, overload, 429, 503, code 1305, etc.).
    pub fn is_transient_api_error(msg: &str) -> bool {
        let lower = msg.to_lowercase();
        // Chinese overload messages
        lower.contains("访问量过大")
            || lower.contains("请稍后再试")
            // Common rate-limit indicators
            || lower.contains("rate limit")
            || lower.contains("rate_limit")
            || lower.contains("too many requests")
            || lower.contains("429")
            || lower.contains("code: 1305")
            // Server overload / temporarily unavailable
            || lower.contains("overloaded")
            || lower.contains("503")
            || lower.contains("service unavailable")
            || lower.contains("please try again")
    }
}

/// Result of the goal runner loop.
#[derive(Debug)]
pub enum GoalOutcome {
    Achieved,
    Error(String),
    /// Token budget exhausted; the loop stopped to avoid overruns.
    UsageLimited { tokens_used: u32, token_budget: u32 },
}

impl std::fmt::Display for GoalOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoalOutcome::Achieved => write!(f, "Goal achieved"),
            GoalOutcome::Error(e) => write!(f, "Goal error: {}", e),
            GoalOutcome::UsageLimited { tokens_used, token_budget } => {
                write!(f, "Token budget exhausted ({}/{})", tokens_used, token_budget)
            }
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
    /// Optional hard cap on total tokens consumed across all iterations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u32>,
    /// Cumulative tokens consumed so far.
    #[serde(default)]
    pub tokens_used: u32,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    /// Optional verification command (e.g. "cargo test") run after each iteration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_command: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub iteration: u32,
    pub timestamp: String,
    /// Short summary of what was accomplished in this iteration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl Default for GoalMeta {
    fn default() -> Self {
        Self {
            iteration: 0,
            tool: "loom".to_string(),
            time_used_seconds: 0,
            token_budget: None,
            tokens_used: 0,
            history: Vec::new(),
            verify_command: None,
        }
    }
}

pub const DEFAULT_MAX_ITERATIONS: u32 = 100;
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;
pub const MAX_HISTORY_ENTRIES: usize = 20;
