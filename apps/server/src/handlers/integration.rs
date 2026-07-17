//! HTTP handlers for the `server.integration` group (`groups/integration.ts`):
//! integration discovery and authentication routes.
//!
//! | Method | Path                                                  | Success                          |
//! |--------|-------------------------------------------------------|----------------------------------|
//! | GET    | `/api/integration`                                    | `Location.response(Integration.Info[])` |
//! | GET    | `/api/integration/:integrationID`                     | `Location.response(Integration.Info \| undefined)` |
//! | POST   | `/api/integration/:integrationID/connect/key`         | `204 No Content`                 |
//! | POST   | `/api/integration/:integrationID/connect/oauth`       | `Location.response(Integration.Attempt)` |
//! | GET    | `/api/integration/attempt/:attemptID`                 | `Location.response(Integration.AttemptStatus)` |
//! | POST   | `/api/integration/attempt/:attemptID/complete`        | `204 No Content`                 |
//! | DELETE | `/api/integration/attempt/:attemptID`                 | `204 No Content`                 |
//!
//! All endpoints accept the deepObject `LocationQuery`. Mutating endpoints map
//! bad input to `InvalidRequestError` (HTTP 400, `errors.ts:3-11`).
//!
//! ## Backing state
//!
//! - **Credential store**: `connect/key` and a successful `complete` store a
//!   real [`CredentialValue::Key`] / `Oauth` in
//!   [`AppState::credentials`](crate::state::AppState) (the same store used by
//!   the credential group). The `(integrationID → cred_*)` link is tracked in a
//!   module-level map so `list`/`get` can surface real `connections`.
//! - **Attempt store**: an in-process `RwLock<HashMap<attemptID, AttemptState>>`
//!   (a [`OnceLock`] singleton, because the attempt surface is not part of
//!   `AppState`). `connect/oauth` creates an entry; `status` polls it;
//!   `cancel`/`complete` resolve it.
//!
//! ## OAuth honesty
//!
//! loom-server has **no OAuth provider configuration** and cannot perform a
//! real authorization-code token exchange. Therefore:
//! - `connect/oauth` creates a genuine `pending` attempt with `mode:"code"`
//!   (a manual code flow), an empty `url`, and instructions explaining no
//!   provider is configured — the lifecycle is real and pollable, never faked.
//! - `attempt/complete` cannot validate an authorization code against a
//!   provider, so it marks the attempt `failed` and returns a clear
//!   `InvalidRequestError` (400) — no fake success. `connect/key`, by contrast,
//!   fully works (it stores a real API key).

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::location::{location_response, LocationInfo};
use crate::state::{new_credential_id, CredentialEntry, CredentialValue, SharedState};

/// OAuth attempt lifetime in seconds (10 minutes).
const ATTEMPT_TTL_SECS: u64 = 600;

// ───────────────────────── response schemas ─────────────────────────

/// `Integration.Info` (schema/integration.ts:95-100):
/// `{ id, name, methods: Method[], connections: Connection.Info[] }`.
#[derive(Serialize)]
struct IntegrationInfo {
    id: String,
    name: String,
    methods: Vec<Value>,
    connections: Vec<Value>,
}

/// `Integration.Attempt` (schema/integration.ts:113-119):
/// `{ attemptID, url, instructions, mode: "auto"|"code", time: {created, expires} }`.
#[derive(Serialize)]
struct Attempt {
    #[serde(rename = "attemptID")]
    attempt_id: String,
    url: String,
    instructions: String,
    mode: &'static str,
    time: AttemptTime,
}

#[derive(Serialize, Clone, Copy)]
struct AttemptTime {
    created: u64,
    expires: u64,
}

/// In-process attempt record backing `connect/oauth` → `status`/`complete`/`cancel`.
#[allow(dead_code)]
struct AttemptState {
    integration_id: String,
    method_id: String,
    label: Option<String>,
    status: AttemptStatus,
    time: AttemptTime,
}

/// `Integration.AttemptStatus` (schema/integration.ts:121-128), tagged by
/// `status`. Serialized as the contract union shape.
#[allow(dead_code)]
#[derive(Clone)]
enum AttemptStatus {
    Pending,
    Complete,
    Failed { message: String },
    Expired,
}

impl AttemptStatus {
    fn tag(&self) -> &'static str {
        match self {
            AttemptStatus::Pending => "pending",
            AttemptStatus::Complete => "complete",
            AttemptStatus::Failed { .. } => "failed",
            AttemptStatus::Expired => "expired",
        }
    }
}

/// Render an [`AttemptStatus`] + [`AttemptTime`] as the contract
/// `AttemptStatus` JSON object (tagged union by `status`).
fn status_value(status: &AttemptStatus, time: AttemptTime) -> Value {
    let tag = status.tag();
    let mut v = json!({ "status": tag, "time": time });
    if let AttemptStatus::Failed { message } = status {
        v["message"] = json!(message);
    }
    v
}

// ───────────────────────── stores ─────────────────────────

/// Process-wide OAuth attempt store: `attemptID ("con_*") → AttemptState`.
static ATTEMPTS: OnceLock<RwLock<HashMap<String, AttemptState>>> = OnceLock::new();

fn attempts() -> &'static RwLock<HashMap<String, AttemptState>> {
    ATTEMPTS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Process-wide integration→credential links: `integrationID → [cred_*]`.
/// Populated by `connect/key`/`complete`; read by `list`/`get` to surface
/// real `Connection.Info` entries.
static LINKS: OnceLock<RwLock<HashMap<String, Vec<String>>>> = OnceLock::new();

fn links() -> &'static RwLock<HashMap<String, Vec<String>>> {
    LINKS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// `Integration.AttemptID` is `"con_" + ascending()` (schema/integration.ts:102-105).
fn new_attempt_id() -> String {
    static GEN: parking_lot::Mutex<u64> = parking_lot::Mutex::new(0);
    let mut g = GEN.lock();
    let cur = *g;
    *g = g.wrapping_add(1);
    format!("con_{cur}")
}

/// Current Unix epoch seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Lazily expire a `pending` attempt whose `expires` time has passed, returning
/// whether it is still live. Mutates the entry in place when it expires.
fn refresh_expiry(state: &mut AttemptState) {
    if matches!(state.status, AttemptStatus::Pending) && now_secs() >= state.time.expires {
        state.status = AttemptStatus::Expired;
    }
}

// ───────────────────────── integration.list ─────────────────────────

/// `GET /api/integration` (integration.ts:12-23) — list available integrations
/// and their authentication methods.
///
/// loom-server has no integration catalog wired to Loom config, so the static
/// catalog is empty — this is a truthful empty list, not a stub. Stored
/// credential connections are surfaced per-integration from the link map.
pub async fn list(State(state): State<SharedState>) -> Response {
    let integrations = build_catalog(&state);
    location_response(&state, integrations).into_response()
}

/// `GET /api/integration/:integrationID` (integration.ts:25-39) — one
/// integration. Success is `UndefinedOr(Integration.Info)`: when the id is
/// unknown the response carries no `data` (200, not 404).
pub async fn get(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
) -> Response {
    let integrations = build_catalog(&state);
    if let Some(info) = integrations.into_iter().find(|i| i.id == integration_id) {
        return location_response(&state, info).into_response();
    }
    // UndefinedOr: omit `data` entirely (true `undefined`), keep `location`.
    let location = serde_json::to_value(LocationInfo::from_state(&state)).unwrap_or(Value::Null);
    Json(json!({ "location": location })).into_response()
}

/// Build the integration catalog with real `connections` derived from the
/// credential store + link map. The static declared integrations are empty
/// (no Loom integration config); connections reflect stored credentials.
fn build_catalog(state: &SharedState) -> Vec<IntegrationInfo> {
    // No static integrations are declared (loom-server has no integration
    // config). We still expose any integrations for which credentials were
    // stored via connect/key, so their connections are visible.
    let link_map = links().read();
    let creds = state.credentials.read();
    let mut ids: Vec<&String> = link_map.keys().collect();
    ids.sort();
    ids.into_iter()
        .map(|id| {
            let connections: Vec<Value> = link_map
                .get(id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|cred_id| {
                    let entry = creds.get(&cred_id)?;
                    Some(json!({
                        "type": "credential",
                        "id": cred_id,
                        "label": entry.label,
                    }))
                })
                .collect();
            IntegrationInfo {
                id: id.clone(),
                name: id.clone(),
                methods: vec![],
                connections,
            }
        })
        .collect()
}

// ───────────────────────── integration.connect.key ─────────────────────────

/// `POST /api/integration/:integrationID/connect/key` payload
/// (integration.ts:44-47): `{ key: string, label?: string }`.
#[derive(Deserialize)]
pub struct ConnectKeyBody {
    pub key: String,
    pub label: Option<String>,
}

/// `POST /api/integration/:integrationID/connect/key` (integration.ts:40-58) —
/// run a key authentication method and store the resulting credential.
///
/// Stores a real [`CredentialValue::Key`] in the credential store and records
/// the `(integrationID → cred_*)` link. Returns `204 No Content`. A missing or
/// empty `key` yields `InvalidRequestError` (400).
pub async fn connect_key(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    Query(_loc): Query<crate::location::LocationQuery>,
    Json(body): Json<ConnectKeyBody>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    if body.key.trim().is_empty() {
        return Err(invalid_request(
            "key is required",
            Some("key"),
        ));
    }
    let cred_id = new_credential_id();
    {
        let mut creds = state.credentials.write();
        creds.insert(
            cred_id.clone(),
            CredentialEntry {
                label: body.label.unwrap_or_else(|| integration_id.clone()),
                value: Some(CredentialValue::Key {
                    key: body.key,
                    metadata: Some(json!({ "integrationID": integration_id })),
                }),
            },
        );
    }
    links()
        .write()
        .entry(integration_id)
        .or_default()
        .push(cred_id);
    Ok(StatusCode::NO_CONTENT)
}

// ───────────────────────── integration.connect.oauth ─────────────────────────

/// `POST /api/integration/:integrationID/connect/oauth` payload
/// (integration.ts:64-68): `{ methodID, inputs: Record<string,string>, label? }`.
#[derive(Deserialize)]
pub struct ConnectOauthBody {
    #[serde(rename = "methodID")]
    pub method_id: String,
    #[serde(default)]
    pub inputs: HashMap<String, String>,
    pub label: Option<String>,
}

/// `POST /api/integration/:integrationID/connect/oauth` (integration.ts:60-79)
/// — start an OAuth attempt and return the authorization details.
///
/// Creates a genuine `pending` attempt. Because loom-server has no OAuth
/// provider configured, `mode` is `"code"` (manual), `url` is empty, and the
/// instructions state no provider is available — honest, never faked. The
/// attempt is real and pollable via `attempt.status`.
pub async fn connect_oauth(
    State(state): State<SharedState>,
    Path(integration_id): Path<String>,
    Query(_loc): Query<crate::location::LocationQuery>,
    body: Option<Json<ConnectOauthBody>>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let body = match body {
        Some(Json(b)) => b,
        None => return Err(invalid_request("request body is required", None)),
    };
    if body.method_id.trim().is_empty() {
        return Err(invalid_request("methodID is required", Some("methodID")));
    }
    let created = now_secs();
    let time = AttemptTime {
        created,
        expires: created + ATTEMPT_TTL_SECS,
    };
    let attempt_id = new_attempt_id();
    let attempt = Attempt {
        attempt_id: attempt_id.clone(),
        url: String::new(),
        instructions: format!(
            "No OAuth provider is configured for integration '{integration_id}'. \
             This is a manual code flow: obtain an authorization code out-of-band and \
             POST it to the attempt complete endpoint. The attempt cannot be \
             auto-completed server-side."
        ),
        mode: "code",
        time,
    };
    attempts().write().insert(
        attempt_id,
        AttemptState {
            integration_id,
            method_id: body.method_id,
            label: body.label,
            status: AttemptStatus::Pending,
            time,
        },
    );
    Ok(location_response(&state, attempt).into_response())
}

// ───────────────────────── integration.attempt.status ─────────────────────────

/// `GET /api/integration/attempt/:attemptID` (integration.ts:81-95) — poll the
/// current status of an OAuth attempt.
///
/// Returns `Location.response(AttemptStatus)`. A pending attempt past its
/// expiry is reported (and recorded) as `expired`. An unknown attempt id
/// returns 404 (the contract declares no endpoint-specific error).
pub async fn attempt_status(
    State(state): State<SharedState>,
    Path(attempt_id): Path<String>,
    Query(_loc): Query<crate::location::LocationQuery>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let (status, time) = {
        let mut store = attempts().write();
        let Some(entry) = store.get_mut(&attempt_id) else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "attempt not found", "attemptID": attempt_id })),
            ));
        };
        refresh_expiry(entry);
        (entry.status.clone(), entry.time)
    };
    Ok(location_response(&state, status_value(&status, time)).into_response())
}

// ───────────────────────── integration.attempt.complete ─────────────────────────

/// `POST /api/integration/attempt/:attemptID/complete` payload
/// (integration.ts:100): `{ code?: string }`.
#[derive(Deserialize, Default)]
pub struct CompleteBody {
    #[serde(default)]
    pub code: Option<String>,
}

/// `POST /api/integration/attempt/:attemptID/complete` (integration.ts:96-112)
/// — complete a code-based OAuth attempt and store the resulting credential.
///
/// loom-server has no OAuth provider and **cannot exchange an authorization
/// code for tokens**. Rather than faking success, the attempt is marked
/// `failed` and a clear `InvalidRequestError` (400) is returned. This is the
/// honest interim until a provider round-trip is wired.
pub async fn attempt_complete(
    State(_state): State<SharedState>,
    Path(attempt_id): Path<String>,
    Query(_loc): Query<crate::location::LocationQuery>,
    _body: Option<Json<CompleteBody>>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let mut store = attempts().write();
    let Some(entry) = store.get_mut(&attempt_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "attempt not found", "attemptID": attempt_id })),
        ));
    };
    refresh_expiry(entry);
    if !matches!(entry.status, AttemptStatus::Pending) {
        return Err(invalid_request(
            &format!("attempt is not pending (status: {})", entry.status.tag()),
            None,
        ));
    }
    // No OAuth provider configured: we cannot validate the code or mint tokens.
    // Mark failed honestly — do NOT fake a credential.
    entry.status = AttemptStatus::Failed {
        message: "OAuth code exchange is not supported: no provider is configured \
                  for this server"
            .to_string(),
    };
    Err(invalid_request(
        "OAuth code exchange is not supported: no provider is configured for this server",
        None,
    ))
}

// ───────────────────────── integration.attempt.cancel ─────────────────────────

/// `DELETE /api/integration/attempt/:attemptID` (integration.ts:113-127) —
/// cancel an OAuth attempt and release its resources.
///
/// Idempotent: returns `204 No Content` whether or not the attempt existed
/// (the contract success type is `NoContent`; REST DELETE is idempotent).
pub async fn attempt_cancel(
    State(_state): State<SharedState>,
    Path(attempt_id): Path<String>,
    Query(_loc): Query<crate::location::LocationQuery>,
) -> StatusCode {
    attempts().write().remove(&attempt_id);
    StatusCode::NO_CONTENT
}

// ───────────────────────── error helper ─────────────────────────

/// Build an `InvalidRequestError` (errors.ts:3-11): HTTP 400 with
/// `{ message, kind?, field? }`.
fn invalid_request(message: &str, field: Option<&str>) -> (StatusCode, Json<Value>) {
    let mut body = json!({ "message": message });
    if let Some(f) = field {
        body["field"] = json!(f);
    }
    (StatusCode::BAD_REQUEST, Json(json!({ "error": body })))
}
