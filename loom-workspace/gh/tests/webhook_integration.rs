//! Integration tests: webhook signature verification, payload parsing, and HTTP POST /webhook.

use gh::{parse_issues_event, verify_signature, webhook_router};
use hmac::Mac;
use tokio::net::TcpListener;

const SECRET: &[u8] = b"test-webhook-secret";
const ISSUES_PAYLOAD: &str = r#"{
  "action": "opened",
  "repository": {
    "id": 1,
    "name": "repo",
    "full_name": "owner/repo",
    "private": false
  },
  "issue": {
    "id": 1,
    "number": 42,
    "title": "Test issue",
    "body": null,
    "state": "open",
    "html_url": "https://github.com/owner/repo/issues/42",
    "labels": []
  }
}"#;

/// Compute X-Hub-Signature-256 header value for (secret, body).
fn sign(secret: &[u8], body: &[u8]) -> String {
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret).expect("key");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(digest))
}

// --- verify_signature ---

#[test]
fn verify_signature_valid() {
    let body = b"{\"action\":\"opened\"}";
    let sig = sign(SECRET, body);
    assert!(verify_signature(SECRET, body, &sig));
}

#[test]
fn verify_signature_invalid_wrong_secret() {
    let body = b"{\"action\":\"opened\"}";
    let sig = sign(SECRET, body);
    assert!(!verify_signature(b"other-secret", body, &sig));
}

#[test]
fn verify_signature_invalid_tampered_body() {
    let body = b"{\"action\":\"opened\"}";
    let sig = sign(SECRET, body);
    assert!(!verify_signature(SECRET, b"{\"action\":\"closed\"}", &sig));
}

#[test]
fn verify_signature_invalid_bad_header_format() {
    let body = b"{}";
    assert!(!verify_signature(SECRET, body, "not-sha256=abc"));
    assert!(!verify_signature(SECRET, body, "sha256=not-hex!!!"));
}

// --- parse_issues_event ---

#[test]
fn parse_issues_event_ok() {
    let ev = parse_issues_event(ISSUES_PAYLOAD.as_bytes()).unwrap();
    assert_eq!(ev.action, "opened");
    assert_eq!(ev.repository.full_name, "owner/repo");
    assert_eq!(ev.issue.number, 42);
    assert_eq!(ev.issue.title, "Test issue");
    assert_eq!(ev.issue.state, "open");
}

#[test]
fn parse_issues_event_invalid_json() {
    let err = parse_issues_event(b"not json").unwrap_err();
    let s = err.to_string();
    assert!(s.contains("parse") || s.contains("Parse"));
}

#[test]
fn parse_issues_event_missing_required_field() {
    let bad = r#"{"action":"opened"}"#;
    let err = parse_issues_event(bad.as_bytes()).unwrap_err();
    assert!(!err.to_string().is_empty());
}

// --- HTTP POST /webhook ---

/// Bind to a random port and spawn the webhook server. Returns (base URL, server join handle).
async fn spawn_webhook_server(secret: &[u8]) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    let app = webhook_router(secret.to_vec());
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (base, handle)
}

#[tokio::test]
async fn webhook_post_missing_sig_returns_401() {
    let (base, _handle) = spawn_webhook_server(SECRET).await;
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/webhook", base))
        .header("X-GitHub-Event", "issues")
        .body(ISSUES_PAYLOAD)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn webhook_post_bad_sig_returns_401() {
    let (base, _handle) = spawn_webhook_server(SECRET).await;
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/webhook", base))
        .header("X-Hub-Signature-256", "sha256=deadbeef")
        .header("X-GitHub-Event", "issues")
        .body(ISSUES_PAYLOAD)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn webhook_post_valid_issues_returns_200() {
    let (base, _handle) = spawn_webhook_server(SECRET).await;
    let body = ISSUES_PAYLOAD.as_bytes();
    let sig = sign(SECRET, body);
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/webhook", base))
        .header("X-Hub-Signature-256", &sig)
        .header("X-GitHub-Event", "issues")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn webhook_post_valid_issues_invalid_json_returns_400() {
    let (base, _handle) = spawn_webhook_server(SECRET).await;
    let body = b"{\"action\":\"opened\"}"; // missing repository, issue
    let sig = sign(SECRET, body);
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/webhook", base))
        .header("X-Hub-Signature-256", &sig)
        .header("X-GitHub-Event", "issues")
        .body(body.as_ref())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
}
