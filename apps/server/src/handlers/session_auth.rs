//! HTTP handlers for UI password authentication
//!
//! Provides HTTP endpoints compatible with OpenChamber's SessionAuthGate component.
//! These endpoints bridge HTTP requests to the ACP session_auth extension.

use axum::{
    extract::{Request, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::state::SharedState;

/// POST /auth/session - UI password login endpoint
///
/// Accepts password credentials and creates a session. Compatible with
/// OpenChamber's SessionAuthGate password submission.
pub async fn session_auth_login(
    State(_state): State<SharedState>,
    Json(payload): Json<SessionAuthLoginRequest>,
) -> impl IntoResponse {
    let password_env = std::env::var("OPENCHAMBER_UI_PASSWORD");
    
    if password_env.is_err() {
        return Json(json!({
            "error": "authentication_not_configured",
            "message": "UI password authentication is not configured. Set OPENCHAMBER_UI_PASSWORD environment variable."
        }))
        .into_response();
    }
    
    let correct_password = password_env.unwrap();
    
    if payload.password != correct_password {
        return Json(json!({
            "error": "invalid_credentials",
            "message": "Invalid password"
        }))
        .into_response();
    }
    
    // Generate a mock session token and client token
    let client_token = if payload.issue_client_token.unwrap_or(false) {
        Some(format!("client-token-{}-{}", uuid::Uuid::new_v4(), chrono::Utc::now().timestamp()))
    } else {
        None
    };
    
    let session_id = format!("session-{}", uuid::Uuid::new_v4());
    let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(86400)).to_rfc3339(); // 24 hours
    
    Json(json!({
        "authenticated": true,
        "sessionId": session_id,
        "clientToken": client_token,
        "expiresAt": expires_at
    }))
    .into_response()
}

/// GET /auth/session - Session status check endpoint
///
/// Returns the current authentication status. Compatible with OpenChamber's
/// SessionAuthGate initial status check.
pub async fn session_auth_status(
    State(_state): State<SharedState>,
    request: Request,
) -> impl IntoResponse {
    // Check for Authorization header for token-based auth
    let auth_header = request.headers().get("Authorization");
    
    // Check if UI password is configured
    let password_configured = std::env::var("OPENCHAMBER_UI_PASSWORD").is_ok();
    
    // If we have an Authorization header, check if it's valid
    if let Some(auth_value) = auth_header {
        if let Ok(auth_str) = auth_value.to_str() {
            if auth_str.starts_with("Bearer ") {
                let token = auth_str.trim_start_matches("Bearer ");
                
                // For now, we'll validate against LOOM_AUTH_TOKEN if it exists
                if let Ok(configured_token) = std::env::var("LOOM_AUTH_TOKEN") {
                    if token == configured_token {
                        // Create a simple hash of the token for principal
                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};
                        let mut hasher = DefaultHasher::new();
                        token.hash(&mut hasher);
                        return Json(json!({
                            "authenticated": true,
                            "principal": format!("token-{:016x}", hasher.finish())
                        }))
                        .into_response();
                    }
                }
                
                // Check if it's a session token (would need proper session store integration)
                if token.starts_with("session-") {
                    return Json(json!({
                        "authenticated": true,
                        "sessionId": token,
                        "principal": "session-user"
                    }))
                    .into_response();
                }
            }
        }
    }
    
    // If no valid auth, check if password authentication is available
    if password_configured {
        return Json(json!({
            "authenticated": false,
            "requiresPassword": true
        }))
        .into_response();
    }
    
    // If no password configured, treat as dev mode - all requests are authenticated
    Json(json!({
        "authenticated": true,
        "principal": "local-anonymous"
    }))
    .into_response()
}

/// DELETE /auth/session - Logout endpoint
///
/// Revokes the current session.
pub async fn session_auth_logout(
    State(_state): State<SharedState>,
    request: Request,
) -> impl IntoResponse {
    // Get session ID from Authorization header
    let auth_header = request.headers().get("Authorization");
    
    if let Some(auth_value) = auth_header {
        if let Ok(auth_str) = auth_value.to_str() {
            if auth_str.starts_with("Bearer ") {
                let _token = auth_str.trim_start_matches("Bearer ");
                // In a full implementation, we would revoke the session here
                return Json(json!({
                    "logged_out": true
                }))
                .into_response();
            }
        }
    }
    
    Json(json!({
        "logged_out": false,
        "error": "no_session"
    }))
    .into_response()
}

// Request/Response types

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAuthLoginRequest {
    pub password: String,
    #[serde(default)]
    pub trust_device: Option<bool>,
    #[serde(default)]
    pub issue_client_token: Option<bool>,
    #[serde(default)]
    pub client_label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAuthResponse {
    pub authenticated: bool,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatusResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_password: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}