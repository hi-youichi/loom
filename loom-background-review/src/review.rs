use crate::prompts::select_review_prompt;
use crate::review_tool_gate::ReviewToolGate;
use agent::agent::{Agent, AgentError, AgentEvent};
use loom_graph::RunnableConfig;
use loom_react_config::ReactBuildConfig;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

/// Minimum length of a system prompt prefix kept byte-exact for prefix-cache parity.
pub const REVIEW_INSTRUCTION: &str = "<background_review>
Review the conversation above and extract durable knowledge.
- Use memory tools to save user preferences and project facts.
- Use skill tools to save reusable task patterns.
- Only use memory and skill tools. Other tools will be denied at runtime.
- If nothing is worth saving, respond with \"Nothing to save.\"
</background_review>";

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
    let kind = match name {
        "memory" => "memory",
        n if n.starts_with("skill_") || n == "skills_list" => "skill",
        _ => "other",
    };
    let preview = if result.len() > 80 {
        format!("{}...", &result[..80])
    } else {
        result.to_string()
    };
    Some(ReviewActionSummary {
        kind: kind.to_string(),
        target: name.to_string(),
        summary: preview,
        succeeded: !result.contains("\"success\": false") && !result.contains("error"),
    })
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

fn build_review_user_message(
    session_content: &str,
    review_memory: bool,
    review_skills: bool,
    max_chars: usize,
) -> Option<String> {
    let prompt = select_review_prompt(review_memory, review_skills)?;
    let truncated = truncate_unicode(session_content, max_chars);
    Some(format!(
        "Here is the conversation to review:\n\n---\n{}\n---\n\n{}",
        truncated, prompt
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
    review_config.call_tool_filter = Some(gate.as_builtin_filter());

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
    let actions_clone = actions.clone();
    let violations_clone = violations.clone();

    info!("Review agent running (thread_id: {})...", fork_thread_id_log);

    let result = agent
        .run_with_config(&user_message, Some(fork_config), move |ev| {
            match ev {
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
                AgentEvent::ToolEnd { name, result, is_error } => {
                    if is_error {
                        warn!("Review tool '{}' failed: {}", name, &result[..result.len().min(200)]);
                        violations_clone
                            .lock()
                            .unwrap()
                            .push(format!("tool '{}' error: {}", name, result));
                    } else {
                        let preview = if result.len() > 120 { format!("{}...", &result[..120]) } else { result.clone() };
                        info!("Review tool '{}' ok: {}", name, preview);
                        if let Some(a) = parse_action(&name, &result) {
                            actions_clone.lock().unwrap().push(a);
                        }
                    }
                }
                _ => {}
            }
        })
        .await?;

    let actions = actions.lock().unwrap().clone();
    let tool_violations = violations.lock().unwrap().clone();
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
pub fn spawn_review_after_session(
    session_id: String,
    model: Option<String>,
) {
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
    fn parse_action_recognizes_skill_tools() {
        for name in ["skills_list", "skill_view", "skill_create", "skill_edit"] {
            let a = parse_action(name, r#"{"ok": true}"#).unwrap();
            assert_eq!(a.kind, "skill", "name: {}", name);
        }
    }

    #[test]
    fn parse_action_flags_failure() {
        let a = parse_action("memory", r#"{"success": false, "error": "denied"}"#).unwrap();
        assert!(!a.succeeded);
    }

    #[test]
    fn parse_action_truncates_long_result() {
        let long = "x".repeat(200);
        let a = parse_action("memory", &long).unwrap();
        assert!(a.summary.len() <= 84);
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
        let mut o = ReviewOutcome::default();
        o.memory_count = 1;
        o.skill_count = 2;
        assert!(o.has_modifications());

        o.memory_count = 0;
        o.skill_count = 0;
        assert!(!o.has_modifications());
    }

    #[test]
    fn review_instruction_mentions_only_memory_and_skill() {
        assert!(REVIEW_INSTRUCTION.contains("memory"));
        assert!(REVIEW_INSTRUCTION.contains("skill"));
        assert!(REVIEW_INSTRUCTION.contains("denied at runtime"));
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
            msg.contains(&suffix),
            "msg should contain the separator suffix"
        );
        assert!(
            !msg.contains(&"x".repeat(200)),
            "msg should not contain the full 10k content; max 100 chars preserved"
        );
        assert!(
            msg.chars().count() < 5000,
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
}
