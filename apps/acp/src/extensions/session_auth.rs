//! `session_auth` extension domain — UI password authentication for Loom Desk
//!
//! Provides password-based session authentication compatible with the Loom Desk frontend.
//! This extension handles password verification, session management, and token issuance.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::auth;
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const ENV_UI_PASSWORD: &str = "LOOMDESK_UI_PASSWORD";
const MAX_PASSWORD_LENGTH: usize = 128;
const MIN_PASSWORD_LENGTH: usize = 4;
const MAX_SESSION_TOKENS: usize = 1000;
const SESSION_TTL_SECONDS: u64 = 24 * 60 * 60; // 24 hours

#[derive(Debug, Clone)]
#[allow(dead_code)] // data-model fields retained for the HTTP/ACP session bridge
pub struct SessionData {
    token: String,
    principal: String,
    created_at: Instant,
    expires_at: Instant,
    trust_device: bool,
    client_token: Option<String>,
}

#[derive(Debug, Default)]
pub struct SessionAuthStore {
    sessions: Mutex<HashMap<String, SessionData>>,
    session_tokens: Mutex<HashMap<String, String>>, // session_id -> token
}

impl SessionAuthStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn validate_password(&self, password: &str) -> bool {
        if let Ok(env_password) = std::env::var(ENV_UI_PASSWORD) {
            return password == env_password;
        }
        false
    }

    pub fn create_session(
        &self,
        token: String,
        principal: String,
        trust_device: bool,
        client_token: Option<String>,
    ) -> Result<String, ExtensionError> {
        let now = Instant::now();
        let expires_at = now + Duration::from_secs(SESSION_TTL_SECONDS);
        let session_id = format!("session-{}", uuid::Uuid::new_v4());

        let session_data = SessionData {
            token: token.clone(),
            principal,
            created_at: now,
            expires_at,
            trust_device,
            client_token,
        };

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| internal_error("session store lock failed"))?;

        if sessions.len() >= MAX_SESSION_TOKENS {
            self.cleanup_expired(&mut sessions)?;
        }

        if sessions.len() >= MAX_SESSION_TOKENS {
            return Err(ExtensionError {
                code: -32002,
                message: "rate_limit".into(),
                data: Some(json!({ "reason": "max_sessions_reached" })),
            });
        }

        let mut session_tokens = self
            .session_tokens
            .lock()
            .map_err(|_| internal_error("session tokens lock failed"))?;

        sessions.insert(session_id.clone(), session_data);
        session_tokens.insert(session_id.clone(), token);

        Ok(session_id)
    }

    pub fn validate_session_token(&self, token: &str) -> Option<SessionData> {
        let mut sessions = self.sessions.lock().ok()?;
        let now = Instant::now();

        let id = sessions
            .iter()
            .find(|(_, session)| session.token == token && session.expires_at > now)
            .map(|(id, _)| id.clone())?;

        let session = sessions.get(&id).cloned();
        if session.is_some() {
            let _ = session_tokens_extend_expiry(&mut sessions, &id, now);
        }
        session
    }

    pub fn get_session_principal(&self, token: &str) -> Option<String> {
        self.validate_session_token(token)
            .map(|session| session.principal)
    }

    pub fn revoke_session(&self, session_id: &str) -> Result<(), ExtensionError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| internal_error("session store lock failed"))?;

        if sessions.remove(session_id).is_some() {
            let mut session_tokens = self
                .session_tokens
                .lock()
                .map_err(|_| internal_error("session tokens lock failed"))?;
            session_tokens.remove(session_id);
            Ok(())
        } else {
            Err(ExtensionError::not_found("session not found"))
        }
    }

    pub fn revoke_all_sessions(&self) -> Result<u32, ExtensionError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| internal_error("session store lock failed"))?;

        let count = sessions.len() as u32;
        sessions.clear();

        let mut session_tokens = self
            .session_tokens
            .lock()
            .map_err(|_| internal_error("session tokens lock failed"))?;
        session_tokens.clear();

        Ok(count)
    }

    pub fn session_count(&self) -> usize {
        self.sessions
            .lock()
            .map(|sessions| sessions.len())
            .unwrap_or(0)
    }

    fn cleanup_expired(
        &self,
        sessions: &mut HashMap<String, SessionData>,
    ) -> Result<(), ExtensionError> {
        let now = Instant::now();
        let expired_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| session.expires_at <= now)
            .map(|(id, _)| id.clone())
            .collect();

        for id in expired_ids {
            sessions.remove(&id);
            if let Ok(mut session_tokens) = self.session_tokens.lock() {
                session_tokens.remove(&id);
            }
        }

        Ok(())
    }
}

fn session_tokens_extend_expiry(
    sessions: &mut HashMap<String, SessionData>,
    session_id: &str,
    now: Instant,
) -> Result<(), ExtensionError> {
    if let Some(session) = sessions.get_mut(session_id) {
        // Extend TTL by 1 hour on activity
        if session.expires_at > now {
            session.expires_at = now + Duration::from_secs(SESSION_TTL_SECONDS);
        }
    }
    Ok(())
}

fn internal_error(message: &str) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(json!(message)),
    }
}

pub struct SessionAuthHandler {
    store: Arc<SessionAuthStore>,
}

impl SessionAuthHandler {
    pub fn new() -> Self {
        Self {
            store: Arc::new(SessionAuthStore::new()),
        }
    }

    pub fn with_store(store: Arc<SessionAuthStore>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Arc<SessionAuthStore> {
        &self.store
    }

    fn check_password_configured(&self) -> Result<(), ExtensionError> {
        if std::env::var(ENV_UI_PASSWORD).is_err() {
            return Err(ExtensionError {
                code: -32002,
                message: "forbidden".into(),
                data: Some(json!({
                    "reason": "password_not_configured",
                    "message": "UI password authentication is not configured. Set LOOMDESK_UI_PASSWORD environment variable."
                })),
            });
        }
        Ok(())
    }
}

impl Default for SessionAuthHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionAuthParams {
    password: String,
    #[serde(default)]
    trust_device: Option<bool>,
    #[serde(default)]
    issue_client_token: Option<bool>,
    #[serde(default)]
    #[allow(dead_code)] // accepted input, reserved for the client-token bridge
    client_label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionAuthResponse {
    authenticated: bool,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_token: Option<String>,
    expires_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionStatusParams {
    #[serde(default)]
    token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionStatusResponse {
    authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    principal: Option<String>,
}

#[async_trait]
impl ExtensionHandler for SessionAuthHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "login" => self.login(params, ctx).await,
            "status" => self.status(params, ctx).await,
            "logout" => self.logout(params, ctx).await,
            "revoke_all" => self.revoke_all(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({
            "login": true,
            "status": true,
            "logout": true,
            "revoke_all": true
        })
    }
}

impl SessionAuthHandler {
    async fn login(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "session_auth", "login")?;
        self.check_password_configured()?;

        let input: SessionAuthParams = serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid login params"))?;

        // Validate password length
        if input.password.len() < MIN_PASSWORD_LENGTH || input.password.len() > MAX_PASSWORD_LENGTH
        {
            return Err(ExtensionError::invalid_params("password length invalid"));
        }

        // Validate password
        if !self.store.validate_password(&input.password) {
            return Err(ExtensionError {
                code: -32002,
                message: "forbidden".into(),
                data: Some(json!({
                    "reason": "invalid_password",
                    "message": "Invalid password"
                })),
            });
        }

        let trust_device = input.trust_device.unwrap_or(false);
        let issue_client_token = input.issue_client_token.unwrap_or(false);

        // Generate session token
        let session_token = format!(
            "session-{}-{}",
            uuid::Uuid::new_v4(),
            chrono::Utc::now().timestamp()
        );

        // Generate principal based on existing connection principal or create new one
        let principal = if ctx.principal.trim().is_empty() {
            format!("user-{}", uuid::Uuid::new_v4())
        } else {
            ctx.principal.clone()
        };

        // Handle client token if requested
        let client_token = if issue_client_token {
            Some(format!(
                "client-token-{}-{}",
                uuid::Uuid::new_v4(),
                chrono::Utc::now().timestamp()
            ))
        } else {
            None
        };

        // Create session
        let session_id = self.store.create_session(
            session_token.clone(),
            principal.clone(),
            trust_device,
            client_token.clone(),
        )?;

        let expires_at = (chrono::Utc::now()
            + chrono::Duration::seconds(SESSION_TTL_SECONDS as i64))
        .to_rfc3339();

        let response = SessionAuthResponse {
            authenticated: true,
            session_id,
            client_token,
            expires_at,
        };

        serde_json::to_value(response)
            .map_err(|_| internal_error("login response serialization failed"))
    }

    async fn status(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        // Session status can be checked without authentication (for the gate)
        let input: SessionStatusParams = serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid status params"))?;

        // Check if a session token was provided
        if let Some(token) = input.token {
            if let Some(session_data) = self.store.validate_session_token(&token) {
                let response = SessionStatusResponse {
                    authenticated: true,
                    session_id: Some(token.clone()),
                    principal: Some(session_data.principal),
                };
                return serde_json::to_value(response)
                    .map_err(|_| internal_error("status response serialization failed"));
            }
        }

        // Check if current connection is already authenticated via principal
        if !ctx.principal.trim().is_empty() {
            let response = SessionStatusResponse {
                authenticated: true,
                session_id: None,
                principal: Some(ctx.principal.clone()),
            };
            return serde_json::to_value(response)
                .map_err(|_| internal_error("status response serialization failed"));
        }

        // Not authenticated
        let response = SessionStatusResponse {
            authenticated: false,
            session_id: None,
            principal: None,
        };

        serde_json::to_value(response)
            .map_err(|_| internal_error("status response serialization failed"))
    }

    async fn logout(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "session_auth", "logout")?;

        let token = params
            .as_str()
            .ok_or_else(|| ExtensionError::invalid_params("token required"))?;

        // Find session by token and revoke it
        let sessions = self
            .store
            .sessions
            .lock()
            .map_err(|_| internal_error("session store lock failed"))?;

        let session_id_to_revoke = sessions
            .iter()
            .find(|(_, session)| session.token == token)
            .map(|(id, _)| id.clone());

        drop(sessions);

        if let Some(session_id) = session_id_to_revoke {
            self.store.revoke_session(&session_id)?;
        }

        Ok(json!({ "logged_out": true }))
    }

    async fn revoke_all(
        &self,
        _params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        auth::check_server_policy(ctx, "session_auth", "revoke_all")?;

        let count = self.store.revoke_all_sessions()?;
        Ok(json!({ "revoked_count": count }))
    }
}
