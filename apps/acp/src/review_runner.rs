//! In-process background review runner for ACP.
//!
//! Spawns `run_review()` in a dedicated OS thread with its own tokio runtime,
//! so the ACP prompt returns immediately while review proceeds in the background.
//! This replaces the CLI path's external subprocess spawn (`loom review session`),
//! which is a no-op under `loom-acp` (the binary has no such subcommand).

use std::path::PathBuf;

use loom_curator::{run_review, ReviewConfig, ReviewHistory, ReviewRecord};
use loom_cli_types::ResolvedModelConfig;
use loom_llm::message::Message;
use loom_react_config::ReactBuildConfig;
use loom_types::state::ReActState;
use tracing::{error, info, warn};

/// Map `/review-skill` scope to `(review_memory, review_skills)` booleans.
pub fn scope_to_review_config(scope: &Option<String>) -> (bool, bool) {
    match scope.as_deref() {
        Some("memory") => (true, false),
        Some("skills") | Some("skill") => (false, true),
        _ => (true, true),
    }
}

/// Build a minimal `ReactBuildConfig` suitable for `run_review()` from the
/// ACP-resolved model configuration. Mirrors `cli/src/review_skill_cmd.rs:40-53`.
fn build_review_react_config(resolved: &ResolvedModelConfig) -> ReactBuildConfig {
    let mut config = ReactBuildConfig::from_env();
    config.openai_api_key = resolved.api_key.clone();
    config.openai_base_url = resolved.base_url.clone();
    config.llm_provider = resolved.provider_type.clone();
    config.model = resolved.model.clone();
    config.working_folder = Some(PathBuf::from("."));
    config
}

/// Extract session conversation as plain text from the SQLite checkpoint store.
///
/// Replicates `cli/src/session.rs:325-348` — queries all checkpoints for the
/// given thread, deserializes `ReActState`, and joins messages as
/// `"User: ...\nAssistant: ...\nTool: ..."`.
fn extract_session_text(thread_id: &str) -> Result<String, String> {
    let db_path = loom_memory::default_memory_db_path();
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("Failed to open database: {e}"))?;
    let mut stmt = conn
        .prepare("SELECT payload FROM checkpoints WHERE thread_id = ?1 ORDER BY metadata_created_at ASC")
        .map_err(|e| format!("Failed to prepare statement: {e}"))?;
    let payloads: Vec<Vec<u8>> = stmt
        .query_map([thread_id], |row| row.get(0))
        .map_err(|e| format!("Query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    if payloads.is_empty() {
        return Err(format!("Session not found: {thread_id}"));
    }

    let mut parts = Vec::new();
    for data in &payloads {
        if let Ok(state) = serde_json::from_slice::<ReActState>(data) {
            for msg in &state.messages {
                match msg {
                    Message::User(u) => parts.push(format!("User: {}", u.as_text())),
                    Message::Assistant(a) => {
                        if !a.content.is_empty() {
                            parts.push(format!("Assistant: {}", a.content));
                        }
                    }
                    Message::Tool { content, .. } => {
                        if let Some(text) = content.as_text() {
                            if !text.is_empty() {
                                parts.push(format!("Tool: {}", text));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(parts.join("\n"))
}

/// Spawn an in-process background review thread.
///
/// Returns immediately. The review runs in a dedicated OS thread with an
/// independent tokio runtime, ensuring:
/// - The ACP prompt is not blocked.
/// - Review failures are logged but never crash the ACP process.
/// - Short sessions are auto-skipped by `run_review`'s `min_session_chars` gate.
pub fn spawn_inprocess_review(
    thread_id: String,
    resolved: ResolvedModelConfig,
    review_memory: bool,
    review_skills: bool,
    trigger: String,
) {
    std::thread::spawn(move || {
        let text = match extract_session_text(&thread_id) {
            Ok(t) => t,
            Err(e) => {
                warn!(thread_id = %thread_id, error = %e, "extract_session_text failed, skipping review");
                return;
            }
        };

        let react_config = build_review_react_config(&resolved);
        let config = ReviewConfig {
            review_memory,
            review_skills,
            ..Default::default()
        };
        let checkpoint_id = uuid::Uuid::new_v4().to_string();

        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                error!(error = %e, "Failed to create review runtime");
                return;
            }
        };

        let start = std::time::Instant::now();
        let result = rt.block_on(run_review(
            react_config,
            checkpoint_id,
            &text,
            &config,
        ));

        let history = ReviewHistory::with_db_path(loom_memory::default_memory_db_path());

        match result {
            Ok(outcome) => {
                let record = ReviewRecord {
                    session_id: thread_id.clone(),
                    reviewed_at: chrono::Utc::now(),
                    trigger: trigger.clone(),
                    model: resolved.model.clone().unwrap_or_default(),
                    memory_update_count: outcome.memory_count,
                    skill_update_count: outcome.skill_count,
                    skipped: outcome.skipped,
                    skip_reason: outcome.skip_reason.clone(),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
                if let Err(e) = history.append(&record) {
                    warn!(thread_id = %thread_id, error = %e, "Failed to persist review record");
                }
                info!(
                    thread_id = %thread_id,
                    skipped = outcome.skipped,
                    memory_count = outcome.memory_count,
                    skill_count = outcome.skill_count,
                    duration_ms = start.elapsed().as_millis() as u64,
                    "ACP in-process review completed"
                );
            }
            Err(e) => {
                let record = ReviewRecord {
                    session_id: thread_id.clone(),
                    reviewed_at: chrono::Utc::now(),
                    trigger: trigger.clone(),
                    model: resolved.model.clone().unwrap_or_default(),
                    memory_update_count: 0,
                    skill_update_count: 0,
                    skipped: true,
                    skip_reason: Some(format!("llm_error: {}", e)),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
                if let Err(persist_err) = history.append(&record) {
                    warn!(thread_id = %thread_id, error = %persist_err, "Failed to persist review record");
                }
                error!(
                    thread_id = %thread_id,
                    error = %e,
                    duration_ms = start.elapsed().as_millis() as u64,
                    "ACP in-process review failed"
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_memory_only() {
        assert_eq!(scope_to_review_config(&Some("memory".into())), (true, false));
    }

    #[test]
    fn scope_skills_only() {
        assert_eq!(scope_to_review_config(&Some("skills".into())), (false, true));
        assert_eq!(scope_to_review_config(&Some("skill".into())), (false, true));
    }

    #[test]
    fn scope_none_defaults_to_both() {
        assert_eq!(scope_to_review_config(&None), (true, true));
    }

    #[test]
    fn scope_unknown_defaults_to_both() {
        assert_eq!(scope_to_review_config(&Some("unknown".into())), (true, true));
    }
}
