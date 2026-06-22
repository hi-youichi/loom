//! Trigger backfill pass — LLM-driven inference of `triggers` for skills
//! that have an empty trigger list.
//!
//! Skills written before the triggers-cleared bug was fixed, or skills created
//! without explicit triggers, accumulate as `triggers: []`. This pass batches
//! them, calls the LLM once per batch, and writes back inferred trigger phrases.
//!
//! ## Usage
//!
//! ```text
//! loom curator backfill-triggers [--dry-run] [--skill <name>] [--batch-size <n>]
//! ```
//!
//! ## LLM call shape
//!
//! No tools are used. The agent receives a plain prompt listing up to
//! `BATCH_SIZE` skills (name + description + body preview), and is asked to
//! return a JSON array of `{"name": "...", "triggers": [...]}` objects.
//! The reply is parsed with a lenient JSON extractor.

use crate::review_tool_gate::ReviewToolGate;
use crate::skill_registry::SkillRegistry;
use agent::agent::{Agent, AgentEvent};
use loom_react_config::ReactBuildConfig;
use serde::Deserialize;
use std::path::Path;
use tracing::{info, warn};

/// Default number of skills per LLM call.
pub const DEFAULT_BATCH_SIZE: usize = 10;

/// Outcome returned by [`run_backfill_triggers`].
#[derive(Debug, Default)]
pub struct BackfillTriggersOutcome {
    /// Skills whose triggers were written (or would be written in dry-run).
    pub updated: Vec<String>,
    /// Skills the LLM produced no usable triggers for.
    pub no_triggers_found: Vec<String>,
    /// Skills that failed to save (skill name → error message).
    pub failed: Vec<(String, String)>,
    pub dry_run: bool,
}

const BACKFILL_SYSTEM_PROMPT: &str = "You are a skill metadata assistant for Loom. \
Your only job is to infer trigger phrases for skills. \
Return ONLY valid JSON — no explanation, no markdown fences.";

fn build_batch_prompt(skills: &[(&str, &str, &str)]) -> String {
    // (name, description, body_preview)
    let mut s = String::from(
        "For each skill below, generate 3–5 short trigger phrases (2–5 words each) \
that describe when a user would need this skill.\n\n",
    );
    for (name, desc, preview) in skills {
        s.push_str(&format!(
            "name: {}\ndescription: {}\nbody_preview: {}\n\n",
            name, desc, preview
        ));
    }
    s.push_str(
        "Return ONLY a JSON array, no other text:\n\
        [{\"name\": \"skill-name\", \"triggers\": [\"trigger1\", \"trigger2\"]}, ...]",
    );
    s
}

#[derive(Deserialize)]
struct SkillTriggerEntry {
    name: String,
    triggers: Vec<String>,
}

fn parse_llm_reply(reply: &str) -> Vec<(String, Vec<String>)> {
    let start = reply.find('[').unwrap_or(0);
    let end = reply.rfind(']').map(|i| i + 1).unwrap_or(reply.len());
    if start >= end {
        return vec![];
    }
    match serde_json::from_str::<Vec<SkillTriggerEntry>>(&reply[start..end]) {
        Ok(entries) => entries
            .into_iter()
            .filter(|e| !e.triggers.is_empty())
            .map(|e| (e.name, e.triggers))
            .collect(),
        Err(e) => {
            warn!("backfill: failed to parse LLM reply as JSON: {}", e);
            vec![]
        }
    }
}

/// Run the trigger backfill pass.
///
/// Scans the skill registry for skills with empty `triggers`, batches them,
/// and calls the LLM to infer trigger phrases. Results are written back unless
/// `dry_run` is true.
///
/// `skill_filter` — when `Some(name)`, only that skill is processed.
/// `batch_size`   — number of skills per LLM call (default: [`DEFAULT_BATCH_SIZE`]).
pub async fn run_backfill_triggers(
    skills_path: &Path,
    agent_config: ReactBuildConfig,
    dry_run: bool,
    skill_filter: Option<&str>,
    batch_size: usize,
) -> Result<BackfillTriggersOutcome, String> {
    let registry = SkillRegistry::new(skills_path);

    let all_meta = registry.list().map_err(|e| e.to_string())?;

    // Load full content for skills with empty triggers, applying optional filter.
    let to_backfill: Vec<_> = all_meta
        .iter()
        .filter(|m| skill_filter.is_none_or(|f| m.name == f))
        .filter(|m| m.triggers.is_empty())
        .filter_map(|m| registry.load(&m.name).ok())
        .collect();

    if to_backfill.is_empty() {
        info!("backfill-triggers: no skills with empty triggers found");
        return Ok(BackfillTriggersOutcome {
            dry_run,
            ..Default::default()
        });
    }

    info!(
        "backfill-triggers: {} skill(s) with empty triggers",
        to_backfill.len()
    );

    let mut outcome = BackfillTriggersOutcome {
        dry_run,
        ..Default::default()
    };

    for batch in to_backfill.chunks(batch_size) {
        let skill_tuples: Vec<(&str, &str, &str)> = batch
            .iter()
            .map(|c| {
                let preview = match c.body.char_indices().nth(300) {
                    Some((i, _)) => &c.body[..i],
                    None => &c.body,
                };
                (c.name.as_str(), c.description.as_str(), preview)
            })
            .collect();

        let prompt = build_batch_prompt(&skill_tuples);

        // Configure agent: no tools, plain completion.
        let gate = ReviewToolGate::with_allowed(Vec::<String>::new());
        let mut config = agent_config.clone();
        config.is_background_review = true;
        config.memory_enabled = false;
        config.user_profile_enabled = false;
        config.memory_nudge_interval = 0;
        config.skill_nudge_interval = 0;
        config.builtin_tool_filter = Some(gate.as_builtin_filter());
        config.call_tool_filter = Some(gate.as_builtin_filter());
        config.extra_tools = None;
        config.system_prompt = Some(BACKFILL_SYSTEM_PROMPT.to_string());

        let agent = Agent::from_config(config)
            .await
            .map_err(|e| format!("Agent build error: {}", e))?;

        let result = agent.run(&prompt, |_: AgentEvent| {}).await;

        let reply = match result {
            Ok(r) => r.reply,
            Err(e) => {
                let names: Vec<String> = batch.iter().map(|c| c.name.clone()).collect();
                warn!(
                    "backfill: LLM call failed for batch {:?}: {}",
                    names, e
                );
                for name in names {
                    outcome.failed.push((name, e.to_string()));
                }
                continue;
            }
        };

        let parsed = parse_llm_reply(&reply);
        let returned_names: std::collections::HashSet<&str> =
            parsed.iter().map(|(n, _)| n.as_str()).collect();

        for skill in batch {
            if !returned_names.contains(skill.name.as_str()) {
                info!("backfill: no triggers returned for '{}'", skill.name);
                outcome.no_triggers_found.push(skill.name.clone());
            }
        }

        for (name, triggers) in parsed {
            if dry_run {
                info!(
                    "backfill (dry-run): '{}' ← {:?}",
                    name, triggers
                );
                outcome.updated.push(name);
                continue;
            }

            match registry.load(&name) {
                Ok(mut content) => {
                    content.triggers = triggers;
                    match registry.save(&name, &content) {
                        Ok(()) => {
                            info!("backfill: updated triggers for '{}'", name);
                            outcome.updated.push(name);
                        }
                        Err(e) => {
                            warn!("backfill: failed to save '{}': {}", name, e);
                            outcome.failed.push((name, e.to_string()));
                        }
                    }
                }
                Err(e) => {
                    warn!("backfill: failed to reload '{}': {}", name, e);
                    outcome.failed.push((name, e.to_string()));
                }
            }
        }
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_llm_reply ──

    #[test]
    fn parse_llm_reply_valid_json_array() {
        let reply = r#"```json
[{"name": "rust-build", "triggers": ["cargo build", "rust compile"]}, {"name": "git-flow", "triggers": ["git merge", "rebase"]}]
```"#;
        let result = parse_llm_reply(reply);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "rust-build");
        assert_eq!(result[0].1, vec!["cargo build", "rust compile"]);
        assert_eq!(result[1].0, "git-flow");
        assert_eq!(result[1].1, vec!["git merge", "rebase"]);
    }

    #[test]
    fn parse_llm_reply_plain_json() {
        let reply = r#"[{"name": "test-skill", "triggers": ["run tests"]}]"#;
        let result = parse_llm_reply(reply);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "test-skill");
        assert_eq!(result[0].1, vec!["run tests"]);
    }

    #[test]
    fn parse_llm_reply_filters_empty_triggers() {
        let reply = r#"[
            {"name": "with-triggers", "triggers": ["a"]},
            {"name": "no-triggers", "triggers": []}
        ]"#;
        let result = parse_llm_reply(reply);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "with-triggers");
    }

    #[test]
    fn parse_llm_reply_invalid_json_returns_empty() {
        let reply = "this is not json at all";
        let result = parse_llm_reply(reply);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_llm_reply_empty_string() {
        let result = parse_llm_reply("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_llm_reply_extracts_array_from_surrounding_text() {
        let reply = "Here are the triggers:\n[{\"name\": \"x\", \"triggers\": [\"y\"]}]\nDone!";
        let result = parse_llm_reply(reply);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "x");
    }

    // ── build_batch_prompt ──

    #[test]
    fn build_batch_prompt_includes_skill_names() {
        let skills = [
            ("skill-a", "Description A", "body preview A"),
            ("skill-b", "Description B", "body preview B"),
        ];
        let prompt = build_batch_prompt(&skills);
        assert!(prompt.contains("skill-a"));
        assert!(prompt.contains("skill-b"));
        assert!(prompt.contains("Description A"));
        assert!(prompt.contains("Description B"));
        assert!(prompt.contains("body preview A"));
        assert!(prompt.contains("body preview B"));
    }

    #[test]
    fn build_batch_prompt_contains_json_instruction() {
        let skills = [("x", "d", "b")];
        let prompt = build_batch_prompt(&skills);
        assert!(prompt.contains("JSON"));
        assert!(prompt.contains("triggers"));
    }

    #[test]
    fn build_batch_prompt_empty_skills() {
        let skills: [(&str, &str, &str); 0] = [];
        let prompt = build_batch_prompt(&skills);
        // Should still contain the instruction, just no skill entries
        assert!(prompt.contains("trigger phrases"));
        assert!(prompt.contains("JSON"));
    }
}
