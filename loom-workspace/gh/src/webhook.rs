//! GitHub webhook signature verification and payload types.
//!
//! All payload structs model the GitHub API / webhook JSON so that every known
//! field is strongly typed. Optional fields use `Option` and `#[serde(default)]`.

use hmac::Mac;
use serde::Deserialize;
use thiserror::Error;

/// Verifies GitHub webhook signature (X-Hub-Signature-256).
/// Uses constant-time comparison to avoid timing attacks.
pub fn verify_signature(secret: &[u8], body: &[u8], signature_header: &str) -> bool {
    let prefix = "sha256=";
    let Some(hex_sig) = signature_header.strip_prefix(prefix) else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_sig) else {
        return false;
    };
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(secret).expect("HMAC accepts any key size");
    mac.update(body);
    let computed = mac.finalize().into_bytes();
    expected.len() == computed.len()
        && subtle::ConstantTimeEq::ct_eq(&expected[..], &computed[..]).into()
}

// -----------------------------------------------------------------------------
// User (sender, issue.user, assignee, etc.)
// -----------------------------------------------------------------------------

/// GitHub user object (simple) from webhook payload.
#[derive(Debug, Clone, Deserialize)]
pub struct SimpleUser {
    pub login: String,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub gravatar_id: Option<String>,
    #[serde(rename = "type", default)]
    pub user_type: Option<String>,
    #[serde(default)]
    pub site_admin: Option<bool>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

// -----------------------------------------------------------------------------
// Repository
// -----------------------------------------------------------------------------

/// GitHub repository object from webhook payload.
#[derive(Debug, Clone, Deserialize)]
pub struct RepoRef {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub owner: Option<SimpleUser>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fork: Option<bool>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub pushed_at: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub stargazers_count: Option<u64>,
    #[serde(default)]
    pub watchers_count: Option<u64>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub has_issues: Option<bool>,
    #[serde(default)]
    pub has_projects: Option<bool>,
    #[serde(default)]
    pub has_downloads: Option<bool>,
    #[serde(default)]
    pub open_issues_count: Option<u64>,
    #[serde(default)]
    pub default_branch: Option<String>,
}

// -----------------------------------------------------------------------------
// Label
// -----------------------------------------------------------------------------

/// Label object from webhook payload.
#[derive(Debug, Clone, Deserialize)]
pub struct LabelPayload {
    pub name: String,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<bool>,
}

// -----------------------------------------------------------------------------
// Milestone (nested in issue)
// -----------------------------------------------------------------------------

/// Milestone object from webhook payload.
#[derive(Debug, Clone, Deserialize)]
pub struct MilestonePayload {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub number: Option<u64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub creator: Option<SimpleUser>,
    #[serde(default)]
    pub open_issues: Option<u64>,
    #[serde(default)]
    pub closed_issues: Option<u64>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub due_on: Option<String>,
    #[serde(default)]
    pub closed_at: Option<String>,
}

// -----------------------------------------------------------------------------
// Pull request ref (when issue is a PR)
// -----------------------------------------------------------------------------

/// Pull request reference on an issue.
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestRef {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub diff_url: Option<String>,
    #[serde(default)]
    pub patch_url: Option<String>,
}

// -----------------------------------------------------------------------------
// Issue
// -----------------------------------------------------------------------------

/// GitHub issue object from webhook payload.
#[derive(Debug, Clone, Deserialize)]
pub struct IssuePayload {
    pub id: u64,
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub state: String,
    pub html_url: String,
    #[serde(default)]
    pub labels: Vec<LabelPayload>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub repository_url: Option<String>,
    #[serde(default)]
    pub labels_url: Option<String>,
    #[serde(default)]
    pub comments_url: Option<String>,
    #[serde(default)]
    pub events_url: Option<String>,
    #[serde(default)]
    pub state_reason: Option<String>,
    #[serde(default)]
    pub user: Option<SimpleUser>,
    #[serde(default)]
    pub assignee: Option<SimpleUser>,
    #[serde(default)]
    pub assignees: Option<Vec<SimpleUser>>,
    #[serde(default)]
    pub milestone: Option<MilestonePayload>,
    #[serde(default)]
    pub locked: Option<bool>,
    #[serde(default)]
    pub active_lock_reason: Option<String>,
    #[serde(default)]
    pub comments: Option<u64>,
    #[serde(default)]
    pub pull_request: Option<PullRequestRef>,
    #[serde(default)]
    pub closed_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub closed_by: Option<SimpleUser>,
    #[serde(default)]
    pub author_association: Option<String>,
}

// -----------------------------------------------------------------------------
// Issues event (top-level)
// -----------------------------------------------------------------------------

/// Top-level payload for "issues" event.
#[derive(Debug, Clone, Deserialize)]
pub struct IssuesEvent {
    pub action: String,
    pub repository: RepoRef,
    pub issue: IssuePayload,
    #[serde(default)]
    pub sender: Option<SimpleUser>,
    /// Present for action "assigned". User who was assigned.
    #[serde(default)]
    pub assignee: Option<SimpleUser>,
    /// Present for action "edited". Describes what changed (e.g. title, body).
    #[serde(default)]
    pub changes: Option<ChangesPayload>,
    /// Enterprise when webhook is configured on an enterprise or org in an enterprise.
    #[serde(default)]
    pub enterprise: Option<EnterprisePayload>,
    /// GitHub App installation when event is sent to an app.
    #[serde(default)]
    pub installation: Option<InstallationPayload>,
    /// Present for action "labeled" or "unlabeled". The label that was added or removed.
    #[serde(default)]
    pub label: Option<LabelPayload>,
    /// Organization when webhook is for an org or repo is owned by an org.
    #[serde(default)]
    pub organization: Option<OrganizationPayload>,
}

/// Describes what changed in an "edited" action (e.g. title, body).
#[derive(Debug, Clone, Deserialize)]
pub struct ChangesPayload {
    #[serde(default)]
    pub title: Option<ChangeFromTo>,
    #[serde(default)]
    pub body: Option<ChangeFromTo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangeFromTo {
    #[serde(default)]
    pub from: Option<String>,
}

/// Minimal enterprise object (webhook context).
#[derive(Debug, Clone, Deserialize)]
pub struct EnterprisePayload {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

/// GitHub App installation (webhook context).
#[derive(Debug, Clone, Deserialize)]
pub struct InstallationPayload {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub app_id: Option<u64>,
}

/// Organization object (webhook context).
#[derive(Debug, Clone, Deserialize)]
pub struct OrganizationPayload {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

// -----------------------------------------------------------------------------
// Backward-compat alias: SenderPayload = SimpleUser (issues event uses SimpleUser for sender)
// -----------------------------------------------------------------------------

/// Alias for user/sender in webhook payloads. Same as [SimpleUser].
pub type SenderPayload = SimpleUser;

// -----------------------------------------------------------------------------
// Error and parser
// -----------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("missing signature header")]
    MissingSignature,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("unsupported event: {0}")]
    UnsupportedEvent(String),
    #[error("parse error: {0}")]
    Parse(String),
}

/// Parse JSON body as issues event. Returns error for other event types or invalid JSON.
pub fn parse_issues_event(json: &[u8]) -> Result<IssuesEvent, WebhookError> {
    serde_json::from_slice(json).map_err(|e| WebhookError::Parse(e.to_string()))
}
