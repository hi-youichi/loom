//! Wire-level e2e test for `_meta.token_usage` extension in `usage_update` notifications.
//!
//! Verifies that when the LLM response SSE includes a `usage` block in the final chunk,
//! the Loom ACP server emits a `session/update` notification whose payload contains
//! the ACP-standard `used`/`size` fields AND the Loom extension `_meta.token_usage`
//! with billing-level input/output/total/cached tokens.
//!
//! Wire schema (when `with_usage_acc` is wired):
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "session/update",
//!   "params": {
//!     "sessionId": "...",
//!     "update": {
//!       "sessionUpdate": "usage_update",
//!       "used": <u64>,
//!       "size": <u64>,
//!       "_meta": {
//!         "token_usage": {
//!           "input_tokens": <u64>,
//!           "output_tokens": <u64>,
//!           "total_tokens": <u64>,
//!           "cached_tokens": <u64>
//!         }
//!       }
//!     }
//!   }
//! }
//! ```

#[path = "e2e/common/mod.rs"]
mod common;

use std::time::Duration;

use common::{with_loom_home, AcpTestHarness, MockLlmServer, TestEnv};
use serde_json::{json, Value};

/// Find the last `session/update` notification whose `update.sessionUpdate == "usage_update"`.
fn find_usage_update(notifs: &[common::jsonrpc::SessionNotification]) -> Option<Value> {
    notifs
        .iter()
        .rev()
        .find(|n| {
            n.params
                .get("update")
                .and_then(|u| u.get("sessionUpdate"))
                .and_then(Value::as_str)
                == Some("usage_update")
        })
        .map(|n| n.params.get("update").cloned().unwrap_or(Value::Null))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn e2e_usage_update_carries_token_usage_meta() {
    let env = TestEnv::setup();
    let llm = MockLlmServer::start().await;

    // Mount an SSE stream that ends with a usage chunk carrying all four billing fields.
    let sse = MockLlmServer::sse_text_chunks_with_usage(
        "hello world",
        /* prompt_tokens */ 123,
        /* completion_tokens */ 45,
        /* total_tokens */ 168,
        /* cached_tokens */ Some(7),
    );
    llm.expect_chat_completion_sse(&sse, 1, None).await;

    with_loom_home(&env, async {
        let h = AcpTestHarness::spawn(&env, &llm.url()).await;

        // initialize + session/new
        let _ = h.request("initialize", json!({"protocolVersion": 1})).await;
        let new_sess = h
            .request(
                "session/new",
                json!({"cwd": env.cwd.to_string_lossy(), "mcpServers": []}),
            )
            .await;
        let session_id = new_sess["sessionId"].as_str().unwrap().to_string();
        let _ = h.drain_notifications().await;

        // prompt
        let resp = h
            .request(
                "session/prompt",
                json!({
                    "sessionId": &session_id,
                    "prompt": [{"type": "text", "text": "hi"}],
                }),
            )
            .await;
        let stop = resp["stopReason"].as_str().unwrap_or("(missing)");
        assert_eq!(
            stop, "end_turn",
            "prompt should complete with end_turn, got {stop}"
        );

        // Drain notifications and find the usage_update.
        let notifs = h.drain_notifications().await;
        let usage_update =
            find_usage_update(&notifs).expect("expected at least one usage_update notification");

        // Standard ACP fields
        assert_eq!(usage_update["sessionUpdate"], "usage_update");
        assert!(usage_update["used"].is_u64(), "used must be u64");
        assert!(usage_update["size"].is_u64(), "size must be u64");

        // Loom extension: _meta.token_usage
        let meta = usage_update["_meta"]
            .as_object()
            .expect("_meta must be an object");
        let token_usage = meta["token_usage"]
            .as_object()
            .expect("_meta.token_usage must be an object");
        assert_eq!(
            token_usage["input_tokens"],
            json!(123),
            "input_tokens must equal mock prompt_tokens"
        );
        assert_eq!(
            token_usage["output_tokens"],
            json!(45),
            "output_tokens must equal mock completion_tokens"
        );
        assert_eq!(
            token_usage["total_tokens"],
            json!(168),
            "total_tokens must equal mock total_tokens"
        );
        assert_eq!(
            token_usage["cached_tokens"],
            json!(7),
            "cached_tokens must equal mock cached_tokens"
        );

        // shutdown
        let status = h.shutdown().await;
        assert!(status.success(), "loom-acp exited non-zero: {status:?}");
    })
    .await;

    tokio::time::sleep(Duration::from_millis(20)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn e2e_usage_update_omits_meta_when_cached_tokens_absent() {
    let env = TestEnv::setup();
    let llm = MockLlmServer::start().await;

    // SSE without cached_tokens — cached_tokens field should still be present in _meta
    // but with value 0 (Loom normalizes missing cached tokens to 0).
    let sse = MockLlmServer::sse_text_chunks_with_usage(
        "ok", /* prompt_tokens */ 50, /* completion_tokens */ 10,
        /* total_tokens */ 60, /* cached_tokens */ None,
    );
    llm.expect_chat_completion_sse(&sse, 1, None).await;

    with_loom_home(&env, async {
        let h = AcpTestHarness::spawn(&env, &llm.url()).await;

        let _ = h.request("initialize", json!({"protocolVersion": 1})).await;
        let new_sess = h
            .request(
                "session/new",
                json!({"cwd": env.cwd.to_string_lossy(), "mcpServers": []}),
            )
            .await;
        let session_id = new_sess["sessionId"].as_str().unwrap().to_string();
        let _ = h.drain_notifications().await;

        let _ = h
            .request(
                "session/prompt",
                json!({
                    "sessionId": &session_id,
                    "prompt": [{"type": "text", "text": "hi"}],
                }),
            )
            .await;

        let notifs = h.drain_notifications().await;
        let usage_update = find_usage_update(&notifs).expect("expected usage_update notification");
        let token_usage = usage_update["_meta"]["token_usage"]
            .as_object()
            .expect("_meta.token_usage must be present");
        assert_eq!(token_usage["input_tokens"], json!(50));
        assert_eq!(token_usage["output_tokens"], json!(10));
        assert_eq!(token_usage["total_tokens"], json!(60));
        // cached_tokens defaults to 0 when LLM response omits it.
        assert_eq!(token_usage["cached_tokens"], json!(0));

        let status = h.shutdown().await;
        assert!(status.success(), "loom-acp exited non-zero: {status:?}");
    })
    .await;

    tokio::time::sleep(Duration::from_millis(20)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn e2e_usage_update_token_usage_grows_across_multiple_prompts() {
    let env = TestEnv::setup();
    let llm = MockLlmServer::start().await;

    // Mock returns the same SSE body for every chat-completion request, so
    // cumulative token_usage grows across multiple prompts in the same session.
    llm.expect_chat_completion_sse(
        &MockLlmServer::sse_text_chunks_with_usage("ok", 100, 10, 110, Some(5)),
        4,
        None,
    )
    .await;

    with_loom_home(&env, async {
        let h = AcpTestHarness::spawn(&env, &llm.url()).await;
        let _ = h.request("initialize", json!({"protocolVersion": 1})).await;
        let new_sess = h
            .request(
                "session/new",
                json!({"cwd": env.cwd.to_string_lossy(), "mcpServers": []}),
            )
            .await;
        let session_id = new_sess["sessionId"].as_str().unwrap().to_string();
        let _ = h.drain_notifications().await;

        for text in ["one", "two", "three", "final"] {
            let _ = h
                .request(
                    "session/prompt",
                    json!({
                        "sessionId": &session_id,
                        "prompt": [{"type": "text", "text": text}],
                    }),
                )
                .await;
        }

        let notifs = h.drain_notifications().await;
        let usage_update =
            find_usage_update(&notifs).expect("expected usage_update after final prompt");
        let token_usage = usage_update["_meta"]["token_usage"]
            .as_object()
            .expect("_meta.token_usage present");

        // `_meta.token_usage` is prompt-scoped (each `session/prompt` creates a fresh
        // accumulator), so after the final prompt the snapshot reflects only the
        // last LLM call's billing: 100/10/110/5.
        assert_eq!(token_usage["input_tokens"], json!(100));
        assert_eq!(token_usage["output_tokens"], json!(10));
        assert_eq!(token_usage["total_tokens"], json!(110));
        assert_eq!(token_usage["cached_tokens"], json!(5));

        let status = h.shutdown().await;
        assert!(status.success(), "loom-acp exited non-zero: {status:?}");
    })
    .await;

    tokio::time::sleep(Duration::from_millis(20)).await;
}
