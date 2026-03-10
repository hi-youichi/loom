//! L3: Webhook handler invokes run entry (mock); assert 200, run called once, RunOptions correct.

use gh::{webhook_router, RunAgentCallback};
use hmac::Mac;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

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

fn sign(secret: &[u8], body: &[u8]) -> String {
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret).expect("key");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(digest))
}

/// Returns (base URL, receiver for RunOptions). Spawns server with mock run_agent.
async fn spawn_webhook_server_with_mock(
    secret: &[u8],
) -> (String, tokio::task::JoinHandle<()>, mpsc::Receiver<loom::RunOptions>) {
    let (tx, rx) = mpsc::channel(2);
    let run_agent: RunAgentCallback = Arc::new(move |opts| {
        let _ = tx.try_send(opts);
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    let app = webhook_router(secret.to_vec(), Some(run_agent));
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (base, handle, rx)
}

#[tokio::test]
async fn webhook_returns_200_then_invokes_run_once() {
    let (base, _handle, mut rx) = spawn_webhook_server_with_mock(SECRET).await;
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
    assert_eq!(res.status(), 200, "webhook must return 200");

    let opts = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout waiting for run_agent call")
        .expect("channel closed");
    assert!(
        opts.message.contains("owner/repo") && opts.message.contains("42") && opts.message.contains("Test issue"),
        "RunOptions.message should contain repo, issue number, title: {}",
        opts.message
    );
    // No second call
    let second = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
    assert!(
        matches!(second, Err(_) | Ok(None)),
        "run_agent should be called once"
    );
}

#[tokio::test]
async fn webhook_run_receives_correct_thread_id() {
    let (base, _handle, mut rx) = spawn_webhook_server_with_mock(SECRET).await;
    let body = ISSUES_PAYLOAD.as_bytes();
    let sig = sign(SECRET, body);
    let client = reqwest::Client::new();
    let _res = client
        .post(format!("{}/webhook", base))
        .header("X-Hub-Signature-256", &sig)
        .header("X-GitHub-Event", "issues")
        .header("X-GitHub-Delivery", "my-delivery-id")
        .body(body)
        .send()
        .await
        .unwrap();
    let opts = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout")
        .expect("one run");
    assert_eq!(opts.thread_id.as_deref(), Some("my-delivery-id"));
}

#[tokio::test]
async fn webhook_invalid_payload_does_not_invoke_run() {
    let (base, _handle, mut rx) = spawn_webhook_server_with_mock(SECRET).await;
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
    let received = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;
    assert!(received.is_err() || received.unwrap().is_none(), "run_agent must not be called");
}

#[tokio::test]
async fn webhook_invalid_signature_does_not_invoke_run() {
    let (base, _handle, mut rx) = spawn_webhook_server_with_mock(SECRET).await;
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
    let received = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await;
    assert!(received.is_err() || received.unwrap().is_none(), "run_agent must not be called");
}
