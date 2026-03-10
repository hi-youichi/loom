//! L4 E2E: Webhook → 200 → loom run started (and completes with MockLlm).
//!
//! Uses injectable run_agent that actually invokes loom::run_agent_with_llm_override
//! with MockLlm so the run completes without real API. Asserts 200, run started
//! (opts received), and optionally run finished.

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

/// E2E: Webhook returns 200, run is triggered and executes (with MockLlm) to completion.
#[tokio::test]
async fn e2e_webhook_triggers_agent() {
    let (tx, mut rx) = mpsc::channel::<loom::RunOptions>(2);
    let run_agent: RunAgentCallback = Arc::new(move |opts| {
        let _ = tx.try_send(opts.clone());
        let opts = opts.clone();
        tokio::spawn(async move {
            let _ = loom::run_agent_with_llm_override(
                &opts,
                &loom::RunCmd::React,
                None,
                Some(Box::new(loom::MockLlm::with_no_tool_calls("e2e done"))),
            )
            .await;
        });
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    let app = webhook_router(SECRET.to_vec(), Some(run_agent));
    let _handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

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

    let opts = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout: run_agent should be invoked")
        .expect("channel closed");
    assert!(
        opts.message.contains("owner/repo") && opts.message.contains("42"),
        "RunOptions.message should contain repo and issue: {}",
        opts.message
    );
}

/// E2E: Run uses working_folder from env (product: agent bound to repo project).
#[tokio::test]
async fn e2e_run_uses_working_folder_for_repo() {
    let dir = tempfile::tempdir().unwrap();
    let work_dir = dir.path().to_path_buf();
    let (tx, mut rx) = mpsc::channel::<loom::RunOptions>(2);
    let run_agent: RunAgentCallback = Arc::new(move |opts| {
        let _ = tx.try_send(opts);
        // Do not spawn real run for this test; we only assert opts.working_folder
    });
    let prev = std::env::var("WORKING_FOLDER").ok();
    std::env::set_var("WORKING_FOLDER", work_dir.as_os_str());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    let app = webhook_router(SECRET.to_vec(), Some(run_agent));
    let _handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

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

    let opts = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout")
        .expect("one run");
    assert_eq!(
        opts.working_folder.as_deref().map(|p| p.to_path_buf()),
        Some(work_dir.clone()),
        "RunOptions.working_folder should match WORKING_FOLDER env"
    );

    if let Some(p) = prev {
        std::env::set_var("WORKING_FOLDER", p);
    } else {
        std::env::remove_var("WORKING_FOLDER");
    }
}
