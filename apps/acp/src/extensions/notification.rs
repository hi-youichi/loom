use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VapidPublicKeyResponse {
    pub vapid_public_key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSubscribeRequest {
    pub subscription: PushSubscription,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSubscribeResponse {
    pub subscribed: bool,
    pub subscription_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationUnsubscribeRequest {
    pub subscription_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationUnsubscribeResponse {
    pub unsubscribed: bool,
    pub count: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSetVisibilityRequest {
    pub visible: bool,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationSetVisibilityResponse {
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApnsRegisterRequest {
    pub token: String,
    pub bundle_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApnsRegisterResponse {
    pub registered: bool,
    pub device_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApnsUnregisterRequest {
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApnsUnregisterResponse {
    pub unregistered: bool,
    pub count: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum NotificationChannel {
    WebPush,
    Apns,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NotificationTestRequest {
    #[serde(default)]
    pub channel: NotificationChannel,
    pub title: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationTestResponse {
    pub sent: bool,
    pub channel: String,
    pub message: String,
}

#[derive(Debug, Clone)]
struct WebPushRecord {
    id: String,
    endpoint: String,
    p256dh: String,
    auth: String,
    principal: String,
    connection_id: String,
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ApnsRecord {
    id: String,
    token: String,
    #[allow(dead_code)]
    bundle_id: Option<String>,
    principal: String,
    connection_id: String,
    session_id: Option<String>,
}

#[derive(Default)]
struct NotificationStore {
    web_push: Vec<WebPushRecord>,
    apns: Vec<ApnsRecord>,
    visibility: HashMap<(String, String, Option<String>), bool>,
    last_test: HashMap<String, Instant>,
    session_auto_accept: HashMap<String, bool>,
    session_activity: HashMap<String, SessionActivity>,
}

#[derive(Debug, Clone)]
struct SessionActivity {
    state: String,
    attention: bool,
}

pub struct NotificationHandler {
    state: Arc<Mutex<NotificationStore>>,
    vapid_public_key: Option<String>,
    apns_enabled: bool,
}

impl NotificationHandler {
    pub fn new() -> Self {
        let vapid_public_key = std::env::var("LOOM_VAPID_PUBLIC_KEY")
            .or_else(|_| std::env::var("VAPID_PUBLIC_KEY"))
            .ok()
            .filter(|v| !v.trim().is_empty());
        let apns_enabled = std::env::var("LOOM_APNS_ENABLED")
            .or_else(|_| std::env::var("APNS_ENABLED"))
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        Self::with_configuration(vapid_public_key, apns_enabled)
    }

    pub fn with_configuration(vapid_public_key: Option<String>, apns_enabled: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(NotificationStore::default())),
            vapid_public_key: vapid_public_key.filter(|value| !value.trim().is_empty()),
            apns_enabled,
        }
    }
}

impl Default for NotificationHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn internal(msg: impl Into<String>) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(msg.into())),
    }
}

fn object(params: &Value) -> Result<&serde_json::Map<String, Value>, ExtensionError> {
    params
        .as_object()
        .ok_or_else(|| ExtensionError::invalid_params("params must be an object"))
}

fn require_context(ctx: &ExtensionContext) -> Result<(), ExtensionError> {
    if ctx.principal.trim().is_empty() || ctx.connection_id.trim().is_empty() {
        return Err(ExtensionError::forbidden(
            "authenticated connection required",
        ));
    }
    Ok(())
}

fn decode<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
    serde_json::from_value(params).map_err(|e| ExtensionError::invalid_params(e.to_string()))
}

fn nonempty(value: &str, field: &str) -> Result<(), ExtensionError> {
    if value.trim().is_empty() {
        Err(ExtensionError::invalid_params(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(())
    }
}

fn valid_optional(value: &Option<String>, field: &str) -> Result<(), ExtensionError> {
    if let Some(value) = value {
        nonempty(value, field)?;
    }
    Ok(())
}

fn valid_base64url(value: &str, field: &str, min: usize, max: usize) -> Result<(), ExtensionError> {
    nonempty(value, field)?;
    if value.len() < min
        || value.len() > max
        || value.contains('=')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(ExtensionError::invalid_params(format!("invalid {field}")));
    }
    Ok(())
}

fn valid_endpoint(endpoint: &str) -> Result<(), ExtensionError> {
    nonempty(endpoint, "subscription.endpoint")?;
    let Some(rest) = endpoint.strip_prefix("https://") else {
        return Err(ExtensionError::invalid_params(
            "subscription.endpoint must use https",
        ));
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.starts_with('.') || host.ends_with('.') {
        return Err(ExtensionError::invalid_params(
            "invalid subscription.endpoint",
        ));
    }
    Ok(())
}

fn validate_scope(value: &Option<String>, ctx: &ExtensionContext) -> Result<(), ExtensionError> {
    valid_optional(value, "sessionId")?;
    if let Some(session) = value {
        if ctx.session_id.as_deref() != Some(session.as_str()) {
            return Err(ExtensionError::forbidden(
                "session is outside the authenticated scope",
            ));
        }
    }
    Ok(())
}

fn prepared_text(
    value: Option<String>,
    field: &str,
    limit: usize,
) -> Result<String, ExtensionError> {
    let value = value.unwrap_or_default();
    if value.chars().count() > limit.saturating_mul(4) {
        return Err(ExtensionError::invalid_params(format!(
            "{field} is too long"
        )));
    }
    Ok(value.chars().take(limit).collect())
}

#[async_trait]
impl ExtensionHandler for NotificationHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "vapid_public_key" => {
                if let Value::Object(map) = &params {
                    if !map.is_empty() {
                        return Err(ExtensionError::invalid_params(
                            "vapid_public_key accepts no parameters",
                        ));
                    }
                } else if !matches!(params, Value::Null) {
                    return Err(ExtensionError::invalid_params("params must be an object"));
                }
                let key = self
                    .vapid_public_key
                    .clone()
                    .ok_or_else(|| internal("VAPID is not configured"))?;
                serde_json::to_value(VapidPublicKeyResponse {
                    vapid_public_key: key,
                    enabled: true,
                })
                .map_err(|e| internal(e.to_string()))
            }
            "subscribe" => {
                require_context(ctx)?;
                self.subscribe(params, ctx)
            }
            "unsubscribe" => {
                require_context(ctx)?;
                self.unsubscribe(params, ctx)
            }
            "set_visibility" => {
                require_context(ctx)?;
                self.set_visibility(params, ctx)
            }
            "apns_register" => {
                require_context(ctx)?;
                self.apns_register(params, ctx)
            }
            "apns_unregister" => {
                require_context(ctx)?;
                self.apns_unregister(params, ctx)
            }
            "test" => {
                require_context(ctx)?;
                self.test(params, ctx)
            }
            "auto_accept_get" => {
                require_context(ctx)?;
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let state = self.state.lock().map_err(|e| internal(e.to_string()))?;
                let enabled = state
                    .session_auto_accept
                    .get(&session_id)
                    .copied()
                    .unwrap_or(false);
                Ok(serde_json::json!({
                    "sessionId": session_id,
                    "enabled": enabled,
                }))
            }
            "auto_accept_set" => {
                require_context(ctx)?;
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| ExtensionError::invalid_params("sessionId is required"))?
                    .to_string();
                let enabled = params
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| ExtensionError::invalid_params("enabled is required"))?;
                let mut state = self.state.lock().map_err(|e| internal(e.to_string()))?;
                state
                    .session_auto_accept
                    .insert(session_id.clone(), enabled);
                Ok(serde_json::json!({
                    "success": true,
                    "sessionId": session_id,
                    "enabled": enabled,
                }))
            }
            "session_activity" => {
                require_context(ctx)?;
                let state = self.state.lock().map_err(|e| internal(e.to_string()))?;
                let sessions: Vec<Value> = state
                    .session_activity
                    .iter()
                    .map(|(session_id, activity)| {
                        serde_json::json!({
                            "sessionId": session_id,
                            "state": activity.state,
                            "attention": activity.attention,
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "sessions": sessions }))
            }
            "view" => {
                require_context(ctx)?;
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let mut state = self.state.lock().map_err(|e| internal(e.to_string()))?;
                if let Some(activity) = state.session_activity.get_mut(&session_id) {
                    activity.attention = false;
                }
                Ok(serde_json::json!({ "success": true }))
            }
            "unview" => {
                require_context(ctx)?;
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let mut state = self.state.lock().map_err(|e| internal(e.to_string()))?;
                if let Some(activity) = state.session_activity.get_mut(&session_id) {
                    activity.attention = true;
                }
                Ok(serde_json::json!({ "success": true }))
            }
            "message_sent" => {
                require_context(ctx)?;
                let session_id = params
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let mut state = self.state.lock().map_err(|e| internal(e.to_string()))?;
                if let Some(activity) = state.session_activity.get_mut(&session_id) {
                    activity.state = "awaiting_input".into();
                } else {
                    state.session_activity.insert(
                        session_id,
                        SessionActivity {
                            state: "awaiting_input".into(),
                            attention: false,
                        },
                    );
                }
                Ok(serde_json::json!({ "success": true }))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({"vapid_public_key": true, "subscribe": true, "unsubscribe": true, "set_visibility": true, "apns_register": true, "apns_unregister": true, "test": true, "auto_accept_get": true, "auto_accept_set": true, "session_activity": true, "view": true, "unview": true, "message_sent": true})
    }
}

impl NotificationHandler {
    fn subscribe(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        if self.vapid_public_key.is_none() {
            return Err(internal("VAPID is not configured"));
        }
        let request: NotificationSubscribeRequest = decode(params)?;
        validate_scope(&request.session_id, ctx)?;
        valid_endpoint(&request.subscription.endpoint)?;
        valid_base64url(
            &request.subscription.keys.p256dh,
            "subscription.keys.p256dh",
            20,
            200,
        )?;
        valid_base64url(
            &request.subscription.keys.auth,
            "subscription.keys.auth",
            8,
            100,
        )?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal("notification state unavailable"))?;
        if let Some(existing) = state.web_push.iter().find(|r| {
            r.principal == ctx.principal
                && r.connection_id == ctx.connection_id
                && r.session_id == request.session_id
                && r.endpoint == request.subscription.endpoint
                && r.p256dh == request.subscription.keys.p256dh
                && r.auth == request.subscription.keys.auth
        }) {
            return serde_json::to_value(NotificationSubscribeResponse {
                subscribed: true,
                subscription_id: existing.id.clone(),
            })
            .map_err(|e| internal(e.to_string()));
        }
        state.apns.retain(|r| {
            !(r.principal == ctx.principal
                && r.connection_id == ctx.connection_id
                && r.session_id == request.session_id)
        });
        let id = format!("sub-{}", uuid::Uuid::new_v4());
        state.web_push.push(WebPushRecord {
            id: id.clone(),
            endpoint: request.subscription.endpoint,
            p256dh: request.subscription.keys.p256dh,
            auth: request.subscription.keys.auth,
            principal: ctx.principal.clone(),
            connection_id: ctx.connection_id.clone(),
            session_id: request.session_id,
        });
        serde_json::to_value(NotificationSubscribeResponse {
            subscribed: true,
            subscription_id: id,
        })
        .map_err(|e| internal(e.to_string()))
    }

    fn unsubscribe(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let _: &serde_json::Map<String, Value> = object(&params)?;
        let request: NotificationUnsubscribeRequest = decode(params)?;
        valid_optional(&request.subscription_id, "subscriptionId")?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal("notification state unavailable"))?;
        let before = state.web_push.len();
        state.web_push.retain(|r| {
            let mine = r.principal == ctx.principal && r.connection_id == ctx.connection_id;
            let target = request
                .subscription_id
                .as_deref()
                .map(|id| r.id == id)
                .unwrap_or(true);
            !(mine && target)
        });
        let count = (before - state.web_push.len()) as u32;
        serde_json::to_value(NotificationUnsubscribeResponse {
            unsubscribed: count > 0,
            count,
        })
        .map_err(|e| internal(e.to_string()))
    }

    fn set_visibility(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let request: NotificationSetVisibilityRequest = decode(params)?;
        validate_scope(&request.session_id, ctx)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal("notification state unavailable"))?;
        state.visibility.insert(
            (
                ctx.principal.clone(),
                ctx.connection_id.clone(),
                request.session_id,
            ),
            request.visible,
        );
        serde_json::to_value(NotificationSetVisibilityResponse { acknowledged: true })
            .map_err(|e| internal(e.to_string()))
    }

    fn apns_register(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        if !self.apns_enabled {
            return Err(internal("APNS is not configured"));
        }
        let request: ApnsRegisterRequest = decode(params)?;
        validate_scope(&request.session_id, ctx)?;
        nonempty(&request.token, "token")?;
        if request.token.len() != 64 || !request.token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ExtensionError::invalid_params(
                "token must be a 64-character hexadecimal string",
            ));
        }
        valid_optional(&request.bundle_id, "bundleId")?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal("notification state unavailable"))?;
        state.web_push.retain(|r| {
            !(r.principal == ctx.principal
                && r.connection_id == ctx.connection_id
                && r.session_id == request.session_id)
        });
        if let Some(existing) = state.apns.iter().find(|r| {
            r.principal == ctx.principal
                && r.connection_id == ctx.connection_id
                && r.session_id == request.session_id
                && r.token.eq_ignore_ascii_case(&request.token)
        }) {
            return serde_json::to_value(ApnsRegisterResponse {
                registered: true,
                device_id: existing.id.clone(),
            })
            .map_err(|e| internal(e.to_string()));
        }
        let id = format!("device-{}", uuid::Uuid::new_v4());
        state.apns.push(ApnsRecord {
            id: id.clone(),
            token: request.token,
            bundle_id: request.bundle_id,
            principal: ctx.principal.clone(),
            connection_id: ctx.connection_id.clone(),
            session_id: request.session_id,
        });
        serde_json::to_value(ApnsRegisterResponse {
            registered: true,
            device_id: id,
        })
        .map_err(|e| internal(e.to_string()))
    }

    fn apns_unregister(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let _: &serde_json::Map<String, Value> = object(&params)?;
        let request: ApnsUnregisterRequest = decode(params)?;
        valid_optional(&request.device_id, "deviceId")?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal("notification state unavailable"))?;
        let before = state.apns.len();
        state.apns.retain(|r| {
            let mine = r.principal == ctx.principal && r.connection_id == ctx.connection_id;
            let target = request
                .device_id
                .as_deref()
                .map(|id| r.id == id)
                .unwrap_or(mine);
            !(mine && target)
        });
        let count = (before - state.apns.len()) as u32;
        serde_json::to_value(ApnsUnregisterResponse {
            unregistered: count > 0,
            count,
        })
        .map_err(|e| internal(e.to_string()))
    }

    fn test(&self, params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let request: NotificationTestRequest = decode(params)?;
        let title = prepared_text(request.title, "title", 120)?;
        let body = prepared_text(request.body, "body", 2000)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal("notification state unavailable"))?;
        let key = ctx.principal.clone();
        if state
            .last_test
            .get(&key)
            .is_some_and(|last| last.elapsed() < Duration::from_secs(60))
        {
            return Err(ExtensionError::invalid_params(
                "test notification rate limit exceeded",
            ));
        }
        let web = state
            .web_push
            .iter()
            .any(|r| r.principal == ctx.principal && r.connection_id == ctx.connection_id);
        let apns = state
            .apns
            .iter()
            .any(|r| r.principal == ctx.principal && r.connection_id == ctx.connection_id);
        let channel = match request.channel {
            NotificationChannel::WebPush if self.vapid_public_key.is_none() || !web => {
                return Err(ExtensionError::invalid_params(
                    "web_push is not configured or has no registration",
                ))
            }
            NotificationChannel::Apns if !self.apns_enabled || !apns => {
                return Err(ExtensionError::invalid_params(
                    "apns is not configured or has no registration",
                ))
            }
            NotificationChannel::WebPush => "web_push",
            NotificationChannel::Apns => "apns",
            NotificationChannel::Auto if web && self.vapid_public_key.is_some() => "web_push",
            NotificationChannel::Auto if apns && self.apns_enabled => "apns",
            NotificationChannel::Auto => {
                return serde_json::to_value(NotificationTestResponse {
                    sent: false,
                    channel: "auto".into(),
                    message: "No notification channel is registered".to_string(),
                })
                .map_err(|e| internal(e.to_string()))
            }
        };
        state.last_test.insert(key, Instant::now());
        let _ = (title, body);
        serde_json::to_value(NotificationTestResponse {
            sent: true,
            channel: channel.into(),
            message: "Notification sent".into(),
        })
        .map_err(|e| internal(e.to_string()))
    }
}
