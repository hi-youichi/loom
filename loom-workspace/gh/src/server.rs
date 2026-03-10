//! Webhook HTTP server: router and handler for use by the binary and integration tests.
//!
//! On valid "issues" events, returns 200 then spawns a loom agent run asynchronously
//! (so GitHub webhook delivery does not time out).
//!
//! When `run_agent` is `Some`, it is called with `RunOptions` instead of spawning the real
//! agent (for tests). When `None`, the real `loom::run_agent_with_options` is used.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use loom::RunOptions;

/// Optional run entry: when set, called with RunOptions instead of spawning the real agent.
pub type RunAgentCallback = Arc<dyn Fn(RunOptions) + Send + Sync>;

/// State shared with the webhook handler.
#[derive(Clone)]
pub struct WebhookAppState {
    pub(crate) webhook_secret: Vec<u8>,
    /// When Some, invoked with RunOptions (tests). When None, real loom run is spawned.
    pub(crate) run_agent: Option<RunAgentCallback>,
}

/// Builds the webhook Router (POST /webhook). Used by the binary and tests.
/// Pass `run_agent: None` for production (real loom); pass `Some(callback)` in tests to capture RunOptions.
pub fn webhook_router(secret: impl Into<Vec<u8>>, run_agent: Option<RunAgentCallback>) -> Router {
    let state = WebhookAppState {
        webhook_secret: secret.into(),
        run_agent,
    };
    Router::new()
        .route("/webhook", post(webhook_handler))
        .with_state(state)
}

async fn webhook_handler(
    State(state): State<WebhookAppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let sig = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok());
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let delivery = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok());

    let Some(sig) = sig else {
        tracing::warn!("webhook missing x-hub-signature-256");
        return (StatusCode::UNAUTHORIZED, "missing signature").into_response();
    };
    if !crate::verify_signature(&state.webhook_secret, body.as_ref(), sig) {
        tracing::warn!(?delivery, "webhook invalid signature");
        return (StatusCode::UNAUTHORIZED, "invalid signature").into_response();
    }

    if event == "issues" {
        match crate::parse_issues_event(body.as_ref()) {
            Ok(ev) => {
                let delivery_id = delivery.map(String::from);
                tracing::info!(
                    delivery = ?delivery_id,
                    action = %ev.action,
                    repo = %ev.repository.full_name,
                    issue = ev.issue.number,
                    title = %ev.issue.title,
                    "issues event"
                );
                let opts = crate::agent::run_options_from_issues_event(&ev, delivery_id.as_deref());
                if let Some(ref run_agent) = state.run_agent {
                    run_agent(opts);
                } else {
                    crate::run_agent::spawn_agent_run(opts);
                }
            }
            Err(e) => {
                tracing::warn!(%e, "parse issues payload failed");
                return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
            }
        }
    } else {
        tracing::info!(?delivery, event = %event, "ignored event type");
    }

    (StatusCode::OK, ()).into_response()
}
