//! GitHub webhook signature verification and payload types.

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
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret).expect("HMAC accepts any key size");
    mac.update(body);
    let computed = mac.finalize().into_bytes();
    expected.len() == computed.len() && subtle::ConstantTimeEq::ct_eq(&expected[..], &computed[..]).into()
}

/// Minimal GitHub repository info from webhook payload.
#[derive(Debug, Clone, Deserialize)]
pub struct RepoRef {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    #[serde(default)]
    pub private: bool,
}

/// Minimal issue info from webhook payload.
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct LabelPayload {
    pub name: String,
}

/// Top-level payload for "issues" event.
#[derive(Debug, Clone, Deserialize)]
pub struct IssuesEvent {
    pub action: String,
    pub repository: RepoRef,
    pub issue: IssuePayload,
    #[serde(default)]
    pub sender: Option<SenderPayload>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SenderPayload {
    pub login: String,
}

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
