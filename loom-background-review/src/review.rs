use crate::review_tool_gate::ReviewToolGate;
use agent::agent::{Agent, AgentError, AgentEvent};
use loom_graph::RunnableConfig;
use loom_react_config::ReactBuildConfig;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

pub const REVIEW_INSTRUCTION: &str = "<background_review>
Review the conversation above and extract durable knowledge.
- Use memory tools to save user preferences and project facts.
- Use skill tools to save reusable task patterns.
- Only use memory and skill tools. Other tools will be denied at runtime.
- If nothing is worth saving, respond with \"Nothing to save.\"
</background_review>";

#[derive(Debug, Clone)]
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
}

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

pub async fn run_review(
    parent_config: ReactBuildConfig,
    parent_thread_id: String,
    parent_checkpoint_id: String,
) -> Result<ReviewOutcome, AgentError> {
    let _ = parent_thread_id;
    let mut review_config = parent_config;
    let gate = ReviewToolGate::new();
    review_config.call_tool_filter = Some(gate.as_builtin_filter());

    let agent = Agent::from_config(review_config).await?;

    let fork_thread_id = format!("review-{}", uuid_v4());
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

    let result = agent
        .run_with_config(REVIEW_INSTRUCTION, Some(fork_config), move |ev| {
            match ev {
                AgentEvent::ToolCallStart { name, .. } => {
                    if !gate.is_allowed(&name) {
                        violations_clone
                            .lock()
                            .unwrap()
                            .push(format!("LLM attempted non-whitelisted tool: {}", name));
                    }
                }
                AgentEvent::ToolEnd { name, result, is_error } => {
                    if is_error {
                        violations_clone
                            .lock()
                            .unwrap()
                            .push(format!("tool '{}' error: {}", name, result));
                    } else if let Some(a) = parse_action(&name, &result) {
                        actions_clone.lock().unwrap().push(a);
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

    Ok(ReviewOutcome {
        actions,
        summary,
        reply: result.reply,
        tool_violations,
    })
}

pub fn spawn_background_review(
    parent_config: ReactBuildConfig,
    parent_thread_id: String,
    parent_checkpoint_id: String,
) {
    tokio::spawn(async move {
        match run_review(parent_config, parent_thread_id, parent_checkpoint_id).await {
            Ok(outcome) => {
                if !outcome.summary.is_empty() {
                    info!("💾 Review: {}", outcome.summary);
                }
                if !outcome.tool_violations.is_empty() {
                    warn!(
                        "Review tool violations ({}): {:?}",
                        outcome.tool_violations.len(),
                        outcome.tool_violations
                    );
                }
            }
            Err(e) => warn!("Review failed: {}", e),
        }
    });
}

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:032x}", nanos)
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
}
