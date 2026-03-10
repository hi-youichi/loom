//! GitHub webhook handling and issue API for Loom agent integration.

pub mod agent;
pub mod issue;
pub mod run_agent;
pub mod server;
pub mod webhook;

pub use agent::run_options_from_issues_event;
pub use issue::{add_labels, close_issue, create_comment, octocrab_from_token, IssueError};
pub use server::{webhook_router, RunAgentCallback};
pub use webhook::{
    parse_issues_event, verify_signature, IssuesEvent, IssuePayload, LabelPayload, RepoRef,
    SenderPayload, WebhookError,
};
