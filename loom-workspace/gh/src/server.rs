//! Webhook HTTP server: router and handler for use by the binary and integration tests.

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};

/// State shared with the webhook handler.
#[derive(Clone)]
pub struct WebhookAppState {
    pub(crate) webhook_secret: Vec<u8>,
}

/// Builds the webhook Router (POST /webhook). Used by the binary and tests.
pub fn webhook_router(secret: impl Into<Vec<u8>>) -> Router {
    let state = WebhookAppState {
        webhook_secret: secret.into(),
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
                tracing::info!(
                    delivery = ?delivery,
                    action = %ev.action,
                    repo = %ev.repository.full_name,
                    issue = ev.issue.number,
                    title = %ev.issue.title,
                    "issues event"
                );
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
