use loom_llm::LlmUsage;

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

/// Tool execution error.
///
/// Used by ActNode to communicate errors back to the LLM for self-correction.
#[derive(Debug, Clone)]
pub enum ToolError {
    /// Generic execution failure (tool not found, runtime error, etc.).
    ExecutionFailed(String),
    Timeout,
    Aborted,
    /// Transient API error (rate-limit, overload) — retryable with backoff.
    RateLimited(String),
    /// Arguments failed JSON validation — LLM should self-correct and retry.
    ///
    /// **Example**: MiniMax-M3 outputs `arguments: "pattern"` instead of
    /// `arguments: "{\"pattern\": \"...\"}"`. We return this error so the LLM
    /// sees the issue and regenerates properly formatted arguments.
    InvalidJsonArguments {
        tool_name: String,
        raw_args: String,
        parse_error: String,
    },
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecutionFailed(msg) => write!(f, "execution failed: {msg}"),
            Self::Timeout => write!(f, "tool execution timed out"),
            Self::Aborted => write!(f, "tool execution aborted"),
            Self::RateLimited(msg) => write!(f, "rate limited: {msg}"),
            Self::InvalidJsonArguments {
                tool_name,
                raw_args,
                parse_error,
            } => {
                // Short preview of raw_args (first 100 chars).
                let preview = raw_args
                    .chars()
                    .take(100)
                    .collect::<String>();
                write!(f,
                    "[{tool_name}] invalid arguments: expected valid JSON object string. \
                     Parse error: {parse_error}. Raw input: {preview}",
                )
            }
        }
    }
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

    /// Returns true if this error indicates the LLM should self-correct.
    ///
    /// When `true`, the agent returns the error to the LLM (instead of silently
    /// falling back) so the model learns to generate valid arguments.
    pub fn is_self_correctable(&self) -> bool {
        matches!(self, Self::InvalidJsonArguments { .. })
    }

    /// Generates an LLM-facing hint for self-correction.
    ///
    /// Returns a message that tells the model what went wrong and how to fix it.
    pub fn self_correct_hint(&self) -> String {
        match self {
            Self::InvalidJsonArguments { tool_name, raw_args, .. } => {
                let preview = raw_args
                    .chars()
                    .take(150)
                    .collect::<String>();
                format!(
                    "[{tool_name}] invalid arguments: expected a JSON object string, \
                     but received: \"{preview}\". \
                     Please provide valid JSON, e.g. {{\"pattern\": \"*.rs\", \"path\": \"src\"}}."
                )
            }
            _ => self.to_string(),
        }
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
