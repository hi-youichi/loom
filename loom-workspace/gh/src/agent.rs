//! Build loom RunOptions from GitHub IssuesEvent for webhook-triggered agent runs.

use std::path::PathBuf;

use crate::webhook::IssuesEvent;
use loom_llm::message::UserContent;

/// Builds `loom::agent_run::RunOptions` from a webhook IssuesEvent so the agent can be run with
/// `loom::agent_run::run_agent_with_options(opts, RunCmd::React, on_event)`.
///
/// - `message`: action, repo, issue number, title, body as natural language.
/// - `thread_id`: `delivery_id` if provided, else `issue-{owner/repo}-{number}` for idempotency.
/// - `working_folder`: from env `WORKING_FOLDER` if set.
/// - `model`: from env `MODEL` or `OPENAI_MODEL` if set.
pub fn run_options_from_issues_event(
    ev: &IssuesEvent,
    delivery_id: Option<&str>,
) -> loom::agent_run::RunOptions {
    let body = ev.issue.body.as_deref().unwrap_or("").trim();
    let message = if body.is_empty() {
        format!(
            "GitHub issue {} in {} #{}: {}",
            ev.action, ev.repository.full_name, ev.issue.number, ev.issue.title
        )
    } else {
        format!(
            "GitHub issue {} in {} #{}: {}\n\n{}",
            ev.action, ev.repository.full_name, ev.issue.number, ev.issue.title, body
        )
    };

    let thread_id = delivery_id.map(String::from).or_else(|| {
        Some(format!(
            "issue-{}-{}",
            ev.repository.full_name, ev.issue.number
        ))
    });

    let working_folder = std::env::var("WORKING_FOLDER").ok().map(PathBuf::from);

    let model = std::env::var("MODEL").ok();

    loom::agent_run::RunOptions {
        message: UserContent::Text(message),
        working_folder,
        session_id: None,
        cancellation: None,
        thread_id,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 120,
        output_json: false,
        model,
        mcp_config_path: None,
        output_timestamp: false,
        dry_run: false,
        debug_llm: false,
        provider: None,
        base_url: None,
        api_key: None,
        provider_type: None,
        any_stream_event_sender: None,
            bash_executor: None,
            extra_tools: None,
            acp_session_id: None,
            force_compact: false,
            chat_id: None,
            worktree: false,
            goal_mode: false,
            acp_mcp_servers: None,
        }
}
