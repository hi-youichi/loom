use crate::prompts::select_review_prompt;
use crate::review_tool_gate::ReviewToolGate;
use agent::agent::{Agent, AgentError, AgentEvent};
use agent::ReactBuildConfig;
use checkpoint::RunnableConfig;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

/// Review instruction appended to the user message of every background-review
/// agent run.
///
/// Tells the LLM that it can only call memory and skill management tools,
/// and that other tools will be denied at runtime.
///
/// **Alignment with Hermes** (`agent/background_review.py:786-790`):
/// `prompt + "\n\nYou can only call memory and skill management tools.
/// Other tools will be denied at runtime - do not attempt them."`
pub const REVIEW_INSTRUCTION: &str = "\
You can only call memory and skill management tools. Other tools will be denied at runtime - do not attempt them.";

/// Input configuration for a single review invocation.
#[derive(Debug, Clone)]
pub struct ReviewConfig {
    pub review_memory: bool,
    pub review_skills: bool,
    pub max_session_chars: usize,
    pub min_session_chars: usize,
    pub observability_enabled: bool,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            review_memory: true,
            review_skills: true,
            max_session_chars: 24_000,
            min_session_chars: 200,
            observability_enabled: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewActionSummary {
    pub kind: String,
    pub target: String,
    /// Human-readable detail about the action. For successes, the tool's own
    /// `message` field (e.g. "Skill 'foo' created."); for failures, the tool's
    /// `error` field (falling back to `message`). Capped at 160 characters with
    /// UTF-8 safe truncation (Chinese text safe).
    pub summary: String,
    pub succeeded: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ReviewOutcome {
    pub actions: Vec<ReviewActionSummary>,
    pub summary: String,
    pub reply: String,
    pub tool_violations: Vec<String>,
    pub memory_count: usize,
    pub skill_count: usize,
    pub duration_ms: u64,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    /// Aggregated LLM token usage across all `AgentEvent::Usage` events in this run.
    /// Sums `prompt_tokens` / `completion_tokens` across every LLM call (think node
    /// may be invoked multiple times in a multi-step review) and tracks the subset
    /// of `prompt_tokens` served from cache so the CLI can print a hit rate.
    pub tokens: TokenUsageSummary,
}

/// Aggregated token usage for one review run.
///
/// `cached_tokens` is the sum of `prompt_tokens_details.cached_tokens` reported
/// by the provider; it is a subset of `prompt_tokens` (not additional to it).
/// `non_cached_prompt` is derived for display: `prompt_tokens - cached_tokens`.
/// `non_cached_prompt` saturates at 0 to guard against provider inconsistencies
/// where `cached_tokens` occasionally exceeds `prompt_tokens`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct TokenUsageSummary {
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// Number of LLM calls that produced a `Usage` event. Used to label the
    /// output (e.g. "1 LLM call" vs "3 LLM calls").
    pub llm_calls: u32,
}

impl TokenUsageSummary {
    /// Non-cached prompt tokens (`prompt - cached`, clamped at 0).
    pub fn non_cached_prompt(&self) -> u64 {
        self.prompt_tokens.saturating_sub(self.cached_tokens)
    }

    /// Returns `true` when at least one LLM call reported usage.
    pub fn is_empty(&self) -> bool {
        self.llm_calls == 0
    }

    /// Records a single `AgentEvent::Usage`. The provider's per-call `total_tokens`
    /// can diverge from the implied `prompt + completion` on some APIs (e.g. when
    /// there is a `cached_tokens` overcount), so we accumulate the components
    /// rather than re-deriving totals to keep the sums faithful.
    fn record(&mut self, usage: &AgentTokenUsage) {
        self.llm_calls = self.llm_calls.saturating_add(1);
        self.prompt_tokens = self
            .prompt_tokens
            .saturating_add(u64::from(usage.prompt_tokens));
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(u64::from(usage.completion_tokens));
        self.cached_tokens = self
            .cached_tokens
            .saturating_add(u64::from(usage.cached_tokens.unwrap_or(0)));
        self.total_tokens = self
            .total_tokens
            .saturating_add(u64::from(usage.total_tokens));
    }
}

/// Lightweight view of an `AgentEvent::Usage` so this module doesn't have to
/// destructure the full event.
struct AgentTokenUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
    cached_tokens: Option<u32>,
}

impl ReviewOutcome {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            skipped: true,
            skip_reason: Some(reason.into()),
            ..Self::default()
        }
    }

    pub fn has_modifications(&self) -> bool {
        self.memory_count + self.skill_count > 0
    }
}

#[derive(Default, Clone)]
pub struct ReviewCallbacks {
    pub on_output: Option<ReviewCallbackFn>,
    pub on_review_complete: Option<ReviewCallbackFn>,
}

pub type ReviewCallbackFn = Arc<dyn Fn(&str) + Send + Sync>;

fn parse_action(name: &str, result: &str) -> Option<ReviewActionSummary> {
    // Alignment with Hermes `summarize_background_review_actions` (background_review.py:237-297):
    // Hermes parses tool message content as JSON and checks `data["success"]`.
    // Loom mirrors this but works at the event level (tool name + result string)
    // rather than the message-snapshot level.
    let kind = match name {
        "memory" => "memory",
        "skill_manage" => "skill",
        "skill_list" | "skill_view" => return None,
        n if n.starts_with("skill_") => "skill",
        _ => "other",
    };

    // Try structured JSON parsing first (aligns with Hermes `data.get("success")`).
    // For successes, surface the tool's `message` field (e.g. "Skill 'foo' created.").
    // For failures, surface the `error` field so the user can see WHY it failed —
    // previously we always read `message`, which made failures look like successes
    // with the generic "updated" placeholder.
    //
    // Fallback chain:
    //   success:  message  →  count  (e.g. "Listed 8 skills")  →  "updated"
    //   failure:  error    →  message                            →  "unknown error"
    //
    // The `count` fallback covers read-only tools like `skill_list` that don't
    // bother to set `message` but do report how many items they listed.
    let (succeeded, detail) = if let Ok(data) = serde_json::from_str::<serde_json::Value>(result) {
        let success = data
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let detail = if success {
            data.get("message")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    data.get("count")
                        .and_then(|v| v.as_u64())
                        .map(|c| format_listed_summary(name, c))
                })
                .unwrap_or_else(|| "updated".to_string())
        } else {
            // Failures: prefer `error`, fall back to `message`, then generic placeholder.
            data.get("error")
                .and_then(|v| v.as_str())
                .or_else(|| data.get("message").and_then(|v| v.as_str()))
                .unwrap_or("unknown error")
                .to_string()
        };
        (success, detail)
    } else {
        // Result is not valid JSON — likely truncated by display_limit or
        // a non-JSON tool output. Skip rather than dump raw text to the user.
        return None;
    };

    // UTF-8 safe truncation: previous code used `&message[..80]` which would panic
    // on Chinese/multibyte text. 160 chars is wide enough for most tool messages
    // while keeping the inline CLI output compact.
    const MAX_DETAIL_CHARS: usize = 160;
    let preview = truncate_unicode(&detail, MAX_DETAIL_CHARS);

    Some(ReviewActionSummary {
        kind: kind.to_string(),
        target: name.to_string(),
        summary: preview,
        succeeded,
    })
}

/// Tool-aware phrasing for read-only tools that report how many items they
/// listed. Currently recognizes `skill_list` ("Listed 8 skills"); other tools
/// fall back to a neutral "Found N items".
fn format_listed_summary(name: &str, count: u64) -> String {
    let (verb, noun) = match name {
        "skill_list" => ("Listed", "skills"),
        "skill_view" => ("Viewed", "skill"),
        _ => ("Found", "items"),
    };
    format!("{} {} {}", verb, count, noun)
}

fn truncate_unicode(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let mut out = String::with_capacity(max_chars * 4);
    for c in s.chars().take(max_chars) {
        out.push(c);
    }
    out
}

/// Extracts the meaningful error reason from a tool result string.
///
/// Tool errors follow the template (see `act_utils.rs`):
///   `Error executing tool '{name}' with kwargs {kwargs} with error:\n {error}\n Please fix...`
/// The kwargs portion can be huge (e.g. full skill content for `skill_manage`),
/// burying the actual error. This function extracts just the `{error}` part.
/// Falls back to the full result (truncated) when the template doesn't match.
fn extract_error_reason(result: &str) -> String {
    const MARKER: &str = " with error:\n";
    const SUFFIX: &str = "\n Please fix the error and try again.";
    if let Some(idx) = result.find(MARKER) {
        let after = &result[idx + MARKER.len()..];
        // Strip the trailing " Please fix..." suffix if present.
        let core = after.strip_suffix(SUFFIX).unwrap_or(after);
        let trimmed = core.trim();
        if !trimmed.is_empty() {
            return truncate_unicode(trimmed, 500);
        }
    }
    truncate_unicode(result, 300)
}

fn build_review_user_message(
    session_content: &str,
    review_memory: bool,
    review_skills: bool,
    max_chars: usize,
) -> Option<String> {
    let prompt = select_review_prompt(review_memory, review_skills)?;
    let truncated = truncate_unicode(session_content, max_chars);
    Some(format!(
        "Here is the conversation to review:\n\n---\n{}\n---\n\n{}\n\n{}",
        truncated, prompt, REVIEW_INSTRUCTION
    ))
}

pub async fn run_review(
    parent_config: ReactBuildConfig,
    parent_checkpoint_id: String,
    session_content: &str,
    config: &ReviewConfig,
) -> Result<ReviewOutcome, AgentError> {
    let start = std::time::Instant::now();

    if !config.review_memory && !config.review_skills {
        return Ok(ReviewOutcome {
            skipped: true,
            skip_reason: Some("no review mode enabled".to_string()),
            duration_ms: start.elapsed().as_millis() as u64,
            ..ReviewOutcome::default()
        });
    }

    if session_content.chars().count() < config.min_session_chars {
        return Ok(ReviewOutcome {
            skipped: true,
            skip_reason: Some("session too short".to_string()),
            duration_ms: start.elapsed().as_millis() as u64,
            ..ReviewOutcome::default()
        });
    }

    let user_message = match build_review_user_message(
        session_content,
        config.review_memory,
        config.review_skills,
        config.max_session_chars,
    ) {
        Some(msg) => msg,
        None => {
            return Ok(ReviewOutcome {
                skipped: true,
                skip_reason: Some("no review mode enabled".to_string()),
                duration_ms: start.elapsed().as_millis() as u64,
                ..ReviewOutcome::default()
            });
        }
    };

    let mut review_config = parent_config;
    let gate = ReviewToolGate::new();
    // Dual-layer defense: `call_tool_filter` intercepts at tool-execution time
    // (runtime guard), and `builtin_tool_filter` hides non-whitelisted tools
    // from the LLM's tool list entirely (reduces hallucinated tool calls).
    // This mirrors the proven pattern in `curator_llm.rs:259-261` and
    // `backfill_triggers.rs:159-160`.
    review_config.call_tool_filter = Some(gate.as_builtin_filter());
    review_config.builtin_tool_filter = Some(gate.as_builtin_filter());

    // If an aux model is configured, use it for the review agent instead of the
    // parent's main model. This allows cheaper/faster models for background review.
    // Aligns Hermes `aux_model` / `dev_model` configuration.
    if let Some(ref aux) = review_config.aux_model {
        review_config.model = Some(aux.clone());
    }

    // Mark this agent as a background-review agent so downstream nodes (and any nudge
    // logic) can short-circuit — prevents recursive background reviews.
    // (plan 011-04, aligns Hermes `background_review_runner` `is_background_review=True`.)
    review_config.is_background_review = true;

    // Disable nudges entirely — review agents must never trigger further reviews.
    review_config.memory_nudge_interval = 0;
    review_config.skill_nudge_interval = 0;

    // In skill-only review mode, disable memory writes entirely so the LLM cannot
    // use memory tools even if it tries to call them (belt-and-suspenders with the
    // tool gate, which already filters them out).
    if !config.review_memory {
        review_config.memory_enabled = false;
        review_config.user_profile_enabled = false;
    }
    // In memory-only review mode, disable skill writes.
    if !config.review_skills {
        // Skill tools are already gated out by ReviewToolGate; nothing further needed.
    }

    info!(
        session_chars = session_content.chars().count(),
        prompt_chars = user_message.chars().count(),
        review_memory = config.review_memory,
        review_skills = config.review_skills,
        "Starting background review"
    );

    let agent = Agent::from_config(review_config).await?;

    let fork_thread_id = format!("review-{}", uuid_v4());
    let fork_thread_id_log = fork_thread_id.clone();
    let fork_config = RunnableConfig {
        thread_id: Some(fork_thread_id),
        checkpoint_id: Some(parent_checkpoint_id),
        checkpoint_ns: "background-review".to_string(),
        ..Default::default()
    };

    let actions: Arc<Mutex<Vec<ReviewActionSummary>>> = Arc::new(Mutex::new(Vec::new()));
    let violations: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let tokens: Arc<Mutex<TokenUsageSummary>> = Arc::new(Mutex::new(TokenUsageSummary::default()));
    let actions_clone = actions.clone();
    let violations_clone = violations.clone();
    let tokens_clone = tokens.clone();

    info!(
        "Review agent running (thread_id: {})...",
        fork_thread_id_log
    );

    let result = agent
        .run_with_config(&user_message, Some(fork_config), move |ev| match ev {
            AgentEvent::ToolCallStart { name, .. } => {
                if !gate.is_allowed(&name) {
                    warn!("Review tool violation: '{}' not in whitelist", name);
                    violations_clone
                        .lock()
                        .unwrap()
                        .push(format!("LLM attempted non-whitelisted tool: {}", name));
                } else {
                    info!("Review tool call: {}", name);
                }
            }
            AgentEvent::ToolEnd {
                name,
                result,
                is_error,
            } => {
                if is_error {
                    let preview = truncate_unicode(&result, 200);
                    warn!("Review tool '{}' failed: {}", name, preview);
                    violations_clone.lock().unwrap().push(format!(
                        "tool '{}' error: {}",
                        name,
                        extract_error_reason(&result)
                    ));
                } else {
                    let preview = truncate_unicode(&result, 120);
                    info!(
                        "Review tool '{}' ok: {}{}",
                        name,
                        preview,
                        if result.chars().count() > 120 {
                            "..."
                        } else {
                            ""
                        }
                    );
                    if let Some(a) = parse_action(&name, &result) {
                        actions_clone.lock().unwrap().push(a);
                    }
                }
            }
            AgentEvent::Usage {
                input,
                output,
                reasoning: _,
                cache_read,
                cache_write: _,
            } => {
                tokens_clone.lock().unwrap().record(&AgentTokenUsage {
                    prompt_tokens: input,
                    completion_tokens: output,
                    total_tokens: input + output,
                    cached_tokens: cache_read,
                });
            }
            _ => {}
        })
        .await?;

    let actions = actions.lock().unwrap().clone();
    let tool_violations = violations.lock().unwrap().clone();
    let tokens = tokens.lock().unwrap().clone();
    let summary = actions
        .iter()
        .filter(|a| a.succeeded)
        .map(|a| format!("{}({})", a.kind, a.target))
        .collect::<Vec<_>>()
        .join(" · ");

    let memory_count = actions
        .iter()
        .filter(|a| a.succeeded && a.kind == "memory")
        .count();
    let skill_count = actions
        .iter()
        .filter(|a| a.succeeded && a.kind == "skill")
        .count();

    let duration_ms = start.elapsed().as_millis() as u64;
    info!(
        memory_count,
        skill_count,
        duration_ms,
        violations = tool_violations.len(),
        reply_chars = result.reply.chars().count(),
        prompt_tokens = tokens.prompt_tokens,
        cached_tokens = tokens.cached_tokens,
        completion_tokens = tokens.completion_tokens,
        total_tokens = tokens.total_tokens,
        llm_calls = tokens.llm_calls,
        "Review completed"
    );

    Ok(ReviewOutcome {
        actions,
        summary,
        reply: result.reply,
        tool_violations,
        memory_count,
        skill_count,
        duration_ms,
        skipped: false,
        skip_reason: None,
        tokens,
    })
}

pub fn spawn_background_review(session_id: String, model: Option<String>) {
    use std::process::Stdio;

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to get current exe path: {}", e);
            return;
        }
    };

    info!(
        session_id = %session_id,
        exe = %exe.display(),
        "Spawning background review subprocess"
    );

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["review", "session", &session_id, "--trigger", "background"]);
    if let Some(ref m) = model {
        cmd.args(["--model", m]);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    match cmd.spawn() {
        Ok(child) => {
            info!(
                session_id = %session_id,
                pid = child.id(),
                "Background review subprocess started"
            );
        }
        Err(e) => {
            error!(
                session_id = %session_id,
                "Failed to spawn background review subprocess: {}",
                e
            );
        }
    }
}

pub fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:032x}", nanos)
}

/// High-level integration helper for Phase 3 main-agent integration.
///
/// Spawns a background review after a main-agent session ends. The caller
/// supplies the Agent (for config_snapshot), the thread_id (typically the
/// session id), and the parent_checkpoint_id (the checkpoint the main
/// session used).
pub fn spawn_review_after_session(session_id: String, model: Option<String>) {
    spawn_background_review(session_id, model);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_action_recognizes_memory() {
        let a = parse_action("memory", r#"{"success": true, "result": "saved"}"#).unwrap();
        assert_eq!(a.kind, "memory");
        assert_eq!(a.target, "memory");
        assert!(a.succeeded);
    }

    #[test]
    fn parse_action_recognizes_skill_manage_as_skill() {
        let a = parse_action("skill_manage", r#"{"ok": true}"#).unwrap();
        assert_eq!(a.kind, "skill");
    }

    #[test]
    fn parse_action_skill_list_returns_none() {
        assert!(parse_action("skill_list", r#"{"success": true, "count": 8}"#).is_none());
    }

    #[test]
    fn parse_action_skill_view_returns_none() {
        assert!(parse_action("skill_view", r#"{"success": true, "count": 1}"#).is_none());
    }

    #[test]
    fn parse_action_parses_json_success_field() {
        // Aligns with Hermes `data.get("success")` parsing.
        let a = parse_action(
            "memory",
            r#"{"success": true, "message": "Entry added to memory"}"#,
        )
        .unwrap();
        assert!(a.succeeded);
        assert_eq!(a.summary, "Entry added to memory");
    }

    #[test]
    fn parse_action_json_failure_flag() {
        let a = parse_action(
            "skill_manage",
            r#"{"success": false, "error": "skill not found"}"#,
        )
        .unwrap();
        assert!(!a.succeeded);
        // The error field must be surfaced so the CLI can show WHY it failed.
        assert_eq!(a.summary, "skill not found");
    }

    #[test]
    fn parse_action_flags_failure() {
        let a = parse_action("memory", r#"{"success": false, "error": "denied"}"#).unwrap();
        assert!(!a.succeeded);
        assert_eq!(a.summary, "denied");
    }

    #[test]
    fn parse_action_failure_falls_back_to_message_when_error_missing() {
        // Some tool failures use `message` instead of `error`. Make sure we still
        // surface something useful.
        let a = parse_action(
            "memory",
            r#"{"success": false, "message": "Could not write to USER.md"}"#,
        )
        .unwrap();
        assert!(!a.succeeded);
        assert_eq!(a.summary, "Could not write to USER.md");
    }

    #[test]
    fn parse_action_failure_uses_unknown_error_when_both_missing() {
        let a = parse_action("memory", r#"{"success": false}"#).unwrap();
        assert!(!a.succeeded);
        assert_eq!(a.summary, "unknown error");
    }

    #[test]
    fn parse_action_count_fallback_for_generic_tool() {
        // The count fallback still works for non-read-only tools.
        let a = parse_action("memory", r#"{"success": true, "count": 3}"#).unwrap();
        assert!(a.succeeded);
        assert_eq!(a.summary, "Found 3 items");
    }

    #[test]
    fn parse_action_message_field_wins_over_count() {
        // If both `message` and `count` are present, prefer `message` (more
        // specific). The count fallback only kicks in when message is absent.
        let a = parse_action(
            "memory",
            r#"{"success": true, "message": "saved", "count": 8}"#,
        )
        .unwrap();
        assert_eq!(a.summary, "saved");
    }

    #[test]
    fn parse_action_count_fallback_does_not_apply_to_failures() {
        // For failures, the count fallback must NOT trigger — we need the error
        // reason, not "Found N items".
        let a = parse_action(
            "memory",
            r#"{"success": false, "error": "dir missing", "count": 0}"#,
        )
        .unwrap();
        assert!(!a.succeeded);
        assert_eq!(a.summary, "dir missing");
    }

    #[test]
    fn format_listed_summary_table_driven() {
        // Lock the verb/noun mapping so future changes are intentional.
        assert_eq!(format_listed_summary("skill_list", 8), "Listed 8 skills");
        assert_eq!(format_listed_summary("skill_list", 1), "Listed 1 skills");
        assert_eq!(format_listed_summary("skill_view", 1), "Viewed 1 skill");
        assert_eq!(format_listed_summary("memory", 0), "Found 0 items");
        assert_eq!(format_listed_summary("unknown_tool", 42), "Found 42 items");
    }

    #[test]
    fn parse_action_truncates_long_message() {
        let long_msg = "x".repeat(500);
        let json = format!(r#"{{"success": true, "message": "{}"}}"#, long_msg);
        let a = parse_action("memory", &json).unwrap();
        assert_eq!(a.summary.chars().count(), 160);
    }

    #[test]
    fn parse_action_truncation_is_utf8_safe_chinese() {
        let long_msg = "用户偏好 Rust 2024 edition。".repeat(20);
        let json = format!(r#"{{"success": true, "message": "{}"}}"#, long_msg);
        let a = parse_action("memory", &json).unwrap();
        assert_eq!(a.summary.chars().count(), 160);
        assert!(a.summary.chars().all(|c| c.is_alphanumeric()
            || c == ' '
            || c == '。'
            || c == '，'
            || c == '\''));
    }

    #[test]
    fn parse_action_truncated_json_returns_none() {
        // Simulates display_text truncation cutting JSON mid-stream.
        assert!(
            parse_action("skill_manage", r#"{"success": true, "skills": [{"name": "#).is_none()
        );
        assert!(parse_action("memory", "not json at all").is_none());
    }

    #[test]
    fn review_outcome_default_is_empty() {
        let o = ReviewOutcome::default();
        assert!(o.actions.is_empty());
        assert!(o.summary.is_empty());
        assert!(o.tool_violations.is_empty());
        assert_eq!(o.memory_count, 0);
        assert_eq!(o.skill_count, 0);
    }

    #[test]
    fn review_outcome_skipped_marks_skipped() {
        let o = ReviewOutcome::skipped("session too short");
        assert!(o.skipped);
        assert_eq!(o.skip_reason.as_deref(), Some("session too short"));
        assert!(!o.has_modifications());
    }

    #[test]
    fn review_outcome_has_modifications() {
        let with_actions = ReviewOutcome {
            memory_count: 1,
            skill_count: 2,
            ..Default::default()
        };
        assert!(with_actions.has_modifications());

        let empty = ReviewOutcome::default();
        assert!(!empty.has_modifications());
    }

    #[test]
    fn token_usage_summary_default_is_empty() {
        let t = TokenUsageSummary::default();
        assert!(t.is_empty());
        assert_eq!(t.prompt_tokens, 0);
        assert_eq!(t.cached_tokens, 0);
        assert_eq!(t.completion_tokens, 0);
        assert_eq!(t.total_tokens, 0);
        assert_eq!(t.llm_calls, 0);
        assert_eq!(t.non_cached_prompt(), 0);
    }

    #[test]
    fn token_usage_summary_record_aggregates_multiple_calls() {
        // Simulate a 2-step review where the LLM was called twice; the second
        // call hit the cache for 800 of its 1500 prompt tokens.
        let mut t = TokenUsageSummary::default();
        t.record(&AgentTokenUsage {
            prompt_tokens: 2_000,
            completion_tokens: 100,
            total_tokens: 2_100,
            cached_tokens: Some(1_200),
        });
        t.record(&AgentTokenUsage {
            prompt_tokens: 1_500,
            completion_tokens: 80,
            total_tokens: 1_580,
            cached_tokens: Some(800),
        });
        assert_eq!(t.llm_calls, 2);
        assert_eq!(t.prompt_tokens, 3_500);
        assert_eq!(t.cached_tokens, 2_000);
        assert_eq!(t.completion_tokens, 180);
        assert_eq!(t.total_tokens, 3_680);
        assert_eq!(t.non_cached_prompt(), 1_500);
        assert!(!t.is_empty());
    }

    #[test]
    fn token_usage_summary_record_handles_missing_cached_field() {
        // Providers that don't report cached_tokens (e.g. Anthropic legacy
        // endpoints) leave the field as `None`; we should treat that as 0
        // without panicking and the summary should still aggregate cleanly.
        let mut t = TokenUsageSummary::default();
        t.record(&AgentTokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            cached_tokens: None,
        });
        assert_eq!(t.cached_tokens, 0);
        assert_eq!(t.non_cached_prompt(), 100);
        assert_eq!(t.llm_calls, 1);
    }

    #[test]
    fn token_usage_summary_non_cached_prompt_saturates_at_zero() {
        // Defensive: a provider bug could surface `cached_tokens > prompt_tokens`.
        // `non_cached_prompt` must clamp at 0 so the CLI never prints a negative.
        let mut t = TokenUsageSummary::default();
        t.record(&AgentTokenUsage {
            prompt_tokens: 100,
            completion_tokens: 10,
            total_tokens: 110,
            cached_tokens: Some(150),
        });
        assert_eq!(t.cached_tokens, 150);
        assert_eq!(t.non_cached_prompt(), 0);
    }

    #[test]
    fn review_outcome_default_has_empty_tokens() {
        let o = ReviewOutcome::default();
        assert!(o.tokens.is_empty());
        assert_eq!(o.tokens.prompt_tokens, 0);
    }

    #[test]
    fn review_outcome_skipped_has_empty_tokens() {
        let o = ReviewOutcome::skipped("noop");
        assert!(o.tokens.is_empty());
    }

    #[test]
    fn token_usage_summary_serializes_with_all_fields() {
        // The CLI's `--json` path serializes the summary verbatim, so all five
        // fields (plus llm_calls) must be present in the output. Locking the
        // shape protects downstream parsers.
        let mut t = TokenUsageSummary::default();
        t.record(&AgentTokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cached_tokens: Some(3),
        });
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["prompt_tokens"], 10);
        assert_eq!(json["cached_tokens"], 3);
        assert_eq!(json["completion_tokens"], 5);
        assert_eq!(json["total_tokens"], 15);
        assert_eq!(json["llm_calls"], 1);
    }

    #[test]
    fn review_instruction_mentions_only_memory_and_skill() {
        assert!(REVIEW_INSTRUCTION.contains("memory"));
        assert!(REVIEW_INSTRUCTION.contains("skill"));
        assert!(REVIEW_INSTRUCTION.contains("denied at runtime"));
        assert!(REVIEW_INSTRUCTION.contains("do not attempt"));
    }

    #[test]
    fn uuid_v4_returns_distinct_values() {
        let a = uuid_v4();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = uuid_v4();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn truncate_unicode_handles_ascii() {
        let s = "a".repeat(100);
        let t = truncate_unicode(&s, 50);
        assert_eq!(t.chars().count(), 50);
    }

    #[test]
    fn truncate_unicode_handles_multibyte() {
        let s = "中".repeat(100);
        let t = truncate_unicode(&s, 50);
        assert_eq!(t.chars().count(), 50);
    }

    #[test]
    fn truncate_unicode_zero_returns_empty() {
        let s = "hello";
        assert_eq!(truncate_unicode(s, 0), "");
    }

    #[test]
    fn truncate_unicode_under_limit_unchanged() {
        let s = "hello world";
        assert_eq!(truncate_unicode(s, 100), "hello world");
    }

    #[test]
    fn build_review_user_message_combined() {
        let msg = build_review_user_message("user: hi", true, true, 1000).unwrap();
        assert!(msg.contains("user: hi"));
        assert!(msg.contains("Memory"));
        assert!(msg.contains("Skills"));
    }

    #[test]
    fn build_review_user_message_includes_review_instruction() {
        // Hermes parity: the user message must include the explicit
        // "only memory/skill tools" instruction so the LLM doesn't drift
        // toward non-review tools (e.g. todo_read) advertised by the
        // default ReAct system prompt.
        let msg = build_review_user_message("user: hi", true, true, 1000).unwrap();
        assert!(
            msg.contains(REVIEW_INSTRUCTION),
            "REVIEW_INSTRUCTION should be appended to the user message"
        );
        assert!(msg.contains("You can only call memory and skill"));
        assert!(msg.contains("denied at runtime"));
        assert!(msg.contains("do not attempt"));
    }

    #[test]
    fn build_review_user_message_includes_instruction_in_all_modes() {
        for (memory, skills) in [(true, true), (true, false), (false, true)] {
            let msg = build_review_user_message("user: hi", memory, skills, 1000).unwrap();
            assert!(
                msg.contains(REVIEW_INSTRUCTION),
                "REVIEW_INSTRUCTION missing for memory={}, skills={}",
                memory,
                skills
            );
        }
    }

    #[test]
    fn build_review_user_message_memory_only() {
        let msg = build_review_user_message("user: hi", true, false, 1000).unwrap();
        assert!(msg.contains("user: hi"));
        assert!(msg.contains("memory tool"));
        assert!(!msg.contains("Skills:"));
    }

    #[test]
    fn build_review_user_message_skill_only() {
        let msg = build_review_user_message("user: hi", false, true, 1000).unwrap();
        assert!(msg.contains("user: hi"));
        assert!(msg.contains("skill library"));
    }

    #[test]
    fn build_review_user_message_neither_returns_none() {
        assert!(build_review_user_message("user: hi", false, false, 1000).is_none());
    }

    #[test]
    fn build_review_user_message_truncates_long_content() {
        let long = "x".repeat(10_000);
        let msg = build_review_user_message(&long, true, true, 100).unwrap();
        let prefix = "Here is the conversation to review:\n\n---\n";
        let suffix = "\n---\n\n";
        assert!(
            msg.starts_with(prefix),
            "msg should start with the conversation prefix"
        );
        assert!(
            msg.contains(suffix),
            "msg should contain the separator suffix"
        );
        assert!(
            !msg.contains(&"x".repeat(200)),
            "msg should not contain the full 10k content; max 100 chars preserved"
        );
        assert!(
            msg.chars().count() < 8000,
            "msg should not be absurdly long (got {} chars)",
            msg.chars().count()
        );
    }

    #[test]
    fn review_config_default_sane() {
        let c = ReviewConfig::default();
        assert!(c.review_memory);
        assert!(c.review_skills);
        assert_eq!(c.max_session_chars, 24_000);
        assert_eq!(c.min_session_chars, 200);
    }

    #[test]
    fn extract_error_reason_from_template() {
        // The standard tool error template — kwargs can be huge, but the
        // actual error reason is after " with error:\n".
        let result = "Error executing tool 'skill_manage' with kwargs {\"action\":\"create\",\"content\":\"...very long skill...\"} with error:\n Skill name 'test' already exists\n Please fix the error and try again.";
        let extracted = extract_error_reason(result);
        assert_eq!(extracted, "Skill name 'test' already exists");
    }

    #[test]
    fn extract_error_reason_chinese() {
        let result = "Error executing tool 'memory' with kwargs {} with error:\n 内存已满，无法写入\n Please fix the error and try again.";
        let extracted = extract_error_reason(result);
        assert_eq!(extracted, "内存已满，无法写入");
    }

    #[test]
    fn extract_error_reason_no_marker_truncates() {
        // When the result doesn't follow the template, fall back to truncation.
        let result = "Some unexpected error format without the marker";
        let extracted = extract_error_reason(result);
        assert_eq!(extracted, "Some unexpected error format without the marker");
    }

    #[test]
    fn extract_error_reason_empty_after_marker_falls_back() {
        // Edge case: marker present but nothing after it.
        let result = "Error executing tool 'x' with kwargs {} with error:\n \n Please fix.";
        let extracted = extract_error_reason(result);
        // After trimming, the error part is empty — fall back to full result truncated.
        assert!(!extracted.is_empty());
    }

    #[test]
    fn extract_error_reason_truncates_long_error() {
        let long_error = "E".repeat(1000);
        let result = format!(
            "Error executing tool 'x' with kwargs {{}} with error:\n {}",
            long_error
        );
        let extracted = extract_error_reason(&result);
        assert_eq!(extracted.chars().count(), 500);
    }

    #[test]
    fn parse_action_other_kind_for_unknown_tool() {
        let a = parse_action("bash", r#"{"success": true}"#).unwrap();
        assert_eq!(a.kind, "other");
    }

    #[test]
    fn parse_action_skill_view_returns_none_not_tracked() {
        assert!(parse_action("skill_view", r#"{"success": true}"#).is_none());
    }

    #[tokio::test]
    async fn run_review_skips_when_no_mode_enabled() {
        let config = ReviewConfig {
            review_memory: false,
            review_skills: false,
            ..Default::default()
        };
        let outcome = run_review(
            ReactBuildConfig::default(),
            "checkpoint-skip".to_string(),
            "hello",
            &config,
        )
        .await
        .unwrap();
        assert!(outcome.skipped);
        assert_eq!(
            outcome.skip_reason.as_deref(),
            Some("no review mode enabled")
        );
    }

    #[test]
    fn spawn_background_review_does_not_panic() {
        spawn_background_review("test-bg-review-noop".to_string(), None);
    }

    #[test]
    fn spawn_review_after_session_does_not_panic() {
        spawn_review_after_session(
            "test-after-session-noop".to_string(),
            Some("test-model".to_string()),
        );
    }
}
