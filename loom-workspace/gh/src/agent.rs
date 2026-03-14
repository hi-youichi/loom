//! Build loom RunOptions from GitHub IssuesEvent for webhook-triggered agent runs.

use std::path::PathBuf;

use crate::webhook::IssuesEvent;

/// Builds `loom::RunOptions` from a webhook IssuesEvent so the agent can be run with
/// `loom::run_agent_with_options(opts, RunCmd::React, on_event)`.
///
/// - `message`: action, repo, issue number, title, body as natural language.
/// - `thread_id`: `delivery_id` if provided, else `issue-{owner/repo}-{number}` for idempotency.
/// - `working_folder`: from env `WORKING_FOLDER` if set.
/// - `model`: from env `MODEL` or `OPENAI_MODEL` if set.
pub fn run_options_from_issues_event(
    ev: &IssuesEvent,
    delivery_id: Option<&str>,
) -> loom::RunOptions {
    let body = ev
        .issue
        .body
        .as_deref()
        .unwrap_or("")
        .trim();
    let message = if body.is_empty() {
        format!(
            "GitHub issue {} in {} #{}: {}",
            ev.action,
            ev.repository.full_name,
            ev.issue.number,
            ev.issue.title
        )
    } else {
        format!(
            "GitHub issue {} in {} #{}: {}\n\n{}",
            ev.action,
            ev.repository.full_name,
            ev.issue.number,
            ev.issue.title,
            body
        )
    };

    let thread_id = delivery_id
        .map(String::from)
        .or_else(|| {
            Some(format!(
                "issue-{}-{}",
                ev.repository.full_name,
                ev.issue.number
            ))
        });

    let working_folder = std::env::var("WORKING_FOLDER")
        .ok()
        .map(PathBuf::from);

    let model = std::env::var("MODEL")
        .or_else(|_| std::env::var("OPENAI_MODEL"))
        .ok();

    loom::RunOptions {
        message,
        working_folder,
        session_id: None,
        thread_id,
        role_file: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 120,
        output_json: false,
        model,
        mcp_config_path: None,
    }
}
