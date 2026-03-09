//! GitHub issue API helpers (comments, close, labels) via octocrab.

use octocrab::models::{issues::Comment as IssueComment, issues::Issue, IssueState};
use octocrab::Octocrab;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IssueError {
    #[error("GitHub API error: {0}")]
    Api(#[from] octocrab::Error),
    #[error("invalid repo: expected 'owner/repo'")]
    InvalidRepo,
}

/// Parse "owner/repo" from repository full_name.
fn parse_owner_repo(full_name: &str) -> Result<(&str, &str), IssueError> {
    let (owner, repo) = full_name.split_once('/').ok_or(IssueError::InvalidRepo)?;
    Ok((owner.trim(), repo.trim()))
}

/// Create a comment on an issue.
pub async fn create_comment(
    crab: &Octocrab,
    owner: &str,
    repo: &str,
    issue_number: u64,
    body: &str,
) -> Result<IssueComment, IssueError> {
    let comment = crab
        .issues(owner, repo)
        .create_comment(issue_number, body)
        .await?;
    Ok(comment)
}

/// Close an issue.
pub async fn close_issue(
    crab: &Octocrab,
    owner: &str,
    repo: &str,
    issue_number: u64,
) -> Result<Issue, IssueError> {
    let issue = crab
        .issues(owner, repo)
        .update(issue_number)
        .state(IssueState::Closed)
        .send()
        .await?;
    Ok(issue)
}

/// Add labels to an issue.
pub async fn add_labels(
    crab: &Octocrab,
    owner: &str,
    repo: &str,
    issue_number: u64,
    labels: &[String],
) -> Result<Vec<octocrab::models::Label>, IssueError> {
    let labels = crab
        .issues(owner, repo)
        .add_labels(issue_number, labels)
        .await?;
    Ok(labels)
}

/// Create octocrab instance from a personal access token (e.g. from env GITHUB_TOKEN).
pub fn octocrab_from_token(token: impl Into<String>) -> Result<Octocrab, octocrab::Error> {
    Octocrab::builder().personal_token(token.into()).build()
}

/// Helpers that take repository full_name (e.g. from webhook payload).
impl crate::webhook::IssuesEvent {
    /// Post a comment on the issue from this event.
    pub async fn create_comment(
        &self,
        crab: &Octocrab,
        body: &str,
    ) -> Result<IssueComment, IssueError> {
        let (owner, repo) = parse_owner_repo(&self.repository.full_name)?;
        create_comment(crab, owner, repo, self.issue.number, body).await
    }

    /// Close the issue from this event.
    pub async fn close_issue(&self, crab: &Octocrab) -> Result<Issue, IssueError> {
        let (owner, repo) = parse_owner_repo(&self.repository.full_name)?;
        close_issue(crab, owner, repo, self.issue.number).await
    }

    /// Add labels to the issue from this event.
    pub async fn add_labels(
        &self,
        crab: &Octocrab,
        labels: &[String],
    ) -> Result<Vec<octocrab::models::Label>, IssueError> {
        let (owner, repo) = parse_owner_repo(&self.repository.full_name)?;
        add_labels(crab, owner, repo, self.issue.number, labels).await
    }
}
