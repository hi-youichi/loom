//! In-memory state shared across all routes.
//!
//! Scope expanded vs the v1 baseline:
//!
//! - `GlobalEvent` envelope now carries `project?` / `workspace?` so the
//!   v2 SDK schema (`types.gen.ts:730-820`) accepts us directly without
//!   translation.
//! - `AppState::abort_tokens` map stores a `RunCancellation` per
//!   `SessionID` (task P1.11) so `POST /session/:id/abort` cancels the
//!   right run instead of a global placeholder.
//! - `AppState::event_buffer` is a bounded ring of `GlobalEvent`s for
//!   `GET /api/session/:id/event` incremental replay (task P2.17).
//! - All places that need a `MessageInfo` or `PartInfo` go through the
//!   JSON helpers (`session_info_to_json`, `message_info_to_json`) so the
//!   v2 envelope gain is applied consistently.
//!
//! In-memory only — restart drops everything (unless a [`Store`] is wired in
//! via [`new_server_state_with_store`], in which case mutations are also
//! write-through persisted and loaded on startup).

// Declare the storage module. We can't add `pub mod storage;` to `lib.rs`
// (not in scope for this task), so we anchor it here with `#[path]`.
// The module is private to `state`; types are re-exported below.
#[path = "storage.rs"]
mod storage;

pub use storage::{InMemoryStore, Store as StoreTrait};

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize, Serializer};
use tokio::sync::broadcast;
use tool_core::active_operation::RunCancellation;

/// `sess_*` counter — used by the v2 replay buffer to assign monotonic
/// event ids without depending on `uuid` ordering. Random u64 is good
/// enough; we only need uniqueness within a single SSE session per the
/// spec (`protocols/sse-events.md:75-91`).
static NEXT_EVENT_ID: parking_lot::Mutex<u64> = parking_lot::Mutex::new(1);

/// Serialize `Option<T>` as `T` (when `Some`) or explicit JSON `null`
/// (when `None`). Use via `#[serde(serialize_with = "serialize_optional")]`
/// in place of `#[serde(skip_serializing_if = "Option::is_none")]`
/// whenever the wire consumer is expected to *see* the field, not skip
/// it.
///
/// Why we have this:
///
/// The opencode/openchamber TUI reads nested map keys with
/// `Object.has(info, 'parentID')`, `info.tool.has(...)`, `info.model.has(...)`,
/// etc. When `Option::is_none` makes those fields disappear entirely from the
/// serialized JSON, the lookup target is `undefined` and the first keypress
/// that triggers a SolidJS re-render crashes the TUI with
/// `undefined is not an object (evaluating 'U.has')`. Emitting `null` instead
/// keeps the field present so the JS check sees `Object.has(info, 'parentID')
/// === false` instead of throwing.
pub fn serialize_path_as_string<S>(
    value: &Option<PathInfo>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(pi) => {
            let rel = std::path::Path::new(&pi.cwd)
                .strip_prefix(&pi.root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| pi.cwd.clone());
            serializer.serialize_str(&rel)
        }
        None => serializer.serialize_none(),
    }
}

pub fn serialize_optional<T, S>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    match value {
        Some(v) => serializer.serialize_some(v),
        None => serializer.serialize_none(),
    }
}

/// Shared handle handed to every route via `axum::extract::State`.
pub type SharedState = Arc<AppState>;

/// All mutable state lives behind `RwLock` so handlers can take it
/// without `async` lock contention on the hot path. The SSE broadcast
/// sender is unbounded enough for our 1024-message buffer, and event
/// replay uses a separate bounded ring buffer.
pub struct AppState {
    /// Durable ACP sessions and notification routing for `/acp` reconnects.
    pub acp_hub: Arc<crate::acp_hub::AcpHub>,
    /// `sess_*` → session metadata (matches opencode's v2 `Session.Info` shape).
    pub sessions: RwLock<HashMap<String, SessionInfo>>,
    /// `sess_*` → ordered list of `MessageInfo` (user + assistant turns).
    pub messages: RwLock<HashMap<String, Vec<MessageInfo>>>,
    /// `msg_*` → ordered list of `PartInfo` (text / tool / reasoning).
    ///
    /// Keyed by message id, not session id, because part lookups in the
    /// translator happen per-message during streaming.
    pub parts: RwLock<HashMap<String, Vec<PartInfo>>>,
    /// `msg_*` → currently active text part id.
    pub active_text: RwLock<HashMap<String, String>>,
    /// `msg_*` → reasoning block id → active reasoning part id.
    pub active_reasoning: RwLock<HashMap<String, HashMap<String, String>>>,
    /// Per-session cancellation tokens — populated while a run is
    /// active, removed on completion. `Option<RunCancellation>` keeps the
    /// shape clone-safe across handlers (task P1.11).
    pub abort_tokens: RwLock<HashMap<String, RunCancellation>>,
    /// Broadcast bus for v1 + v2 SSE channels. Capacity 1024 — enough for
    /// minutes of text deltas at typical token rates; drops oldest on overflow
    /// (a slow SSE consumer must resync via `GET /session/:id/messages`).
    pub event_tx: broadcast::Sender<GlobalEvent>,
    /// Replay buffer for `GET /api/session/:id/event` (task P2.17).
    /// Capped at `EVENT_BUFFER_CAP` (≈ 5 minutes of streaming).
    pub event_buffer: RwLock<VecDeque<GlobalEvent>>,
    /// Separate OpenCode v2 event bus. Unlike `event_tx`, session durable
    /// events carry a per-session aggregate sequence.
    pub v2_event_tx: broadcast::Sender<crate::v2_event::V2Event>,
    pub v2_session_events: RwLock<HashMap<String, VecDeque<crate::v2_event::V2Event>>>,
    pub v2_next_seq: RwLock<HashMap<String, u64>>,
    pub v2_reasoning_ids: RwLock<HashMap<String, HashMap<String, String>>>,
    /// Tool inputs already opened by provider-native stream deltas.
    pub v2_started_tool_calls: RwLock<HashSet<(String, String)>>,
    pub v2_terminal_tool_calls: RwLock<HashSet<(String, String)>>,
    /// Materialized compacted context and a staged session revert transaction.
    pub session_contexts: RwLock<HashMap<String, String>>,
    pub staged_reverts: RwLock<HashMap<String, serde_json::Value>>,
    pub v2_file_log: Option<Arc<crate::v2_event::V2FileLog>>,
    pub v2_publish_lock: Mutex<()>,
    /// Global config blob (task P2.23 + P0.2). Read on `GET /config` and
    /// `GET /global/config`; written through `PATCH /global/config`.
    pub config: RwLock<ConfigInfo>,
    /// Persist global config updates when running the production binary.
    pub persist_config: bool,
    /// Project metadata stored on first `/project` request (v2 spec
    /// injects `directory + project + workspace` into the SSE envelope).
    pub project: RwLock<ProjectInfo>,
    /// Permission requests keyed by request ID (task LS-010). Backs the
    /// live `GET /permission` pending list and `permission.asked` /
    /// `permission.replied` event lifecycle.
    pub permissions: RwLock<HashMap<String, PermissionRequest>>,
    /// Optional persistence layer (task LS-013b + LS-014). `None` for tests
    /// (isolated, behavior-identical to the pre-seam code). When `Some`,
    /// mutations are also write-through persisted via the `persist_*` helpers
    /// and loaded on startup via [`load_from_store`].
    pub store: Option<Arc<dyn StoreTrait + Send + Sync>>,
    /// Credential store: `cred_*` → [`CredentialEntry`] (schema-credential.ts,
    /// group-credential.ts). Backs `PATCH/DELETE /api/credential/:credentialID`.
    pub credentials: RwLock<HashMap<String, CredentialEntry>>,
    /// PTY registry: `pty_*` → live [`PtyHandle`] (schema-pty.ts,
    /// group-pty.ts). Backs the `/api/pty[/:ptyID]` CRUD routes and the
    /// WebSocket connect flow.
    pub ptys: RwLock<HashMap<String, PtyHandle>>,
    /// Single-use, short-lived PTY connect tickets: `tkt_*` → [`PtyTicket`]
    /// (group-pty.ts `POST /api/pty/:ptyID/connect-token` →
    /// `GET .../connect?ticket=`).
    pub pty_tickets: RwLock<HashMap<String, PtyTicket>>,
}

const EVENT_BUFFER_CAP: usize = 512;

/// All mutable config lives here. v2 spec (§4.1) defines a `ConfigInfo`
/// shape; we keep our MVP scaffold small and add fields as handlers need them.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ConfigInfo {
    #[serde(default, serialize_with = "serialize_optional")]
    pub theme: Option<String>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub provider: Option<serde_json::Value>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<serde_json::Value>,
    /// Anything extra callers want to stash; opt-in via `_meta`.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub extra: serde_json::Value,
}

/// Project metadata required by the v2 envelope (`GlobalEventSchema`).
#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: String,
    pub worktree: String,
    pub directory: String,
    #[serde(default, serialize_with = "serialize_optional")]
    pub vcs: Option<serde_json::Value>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub workspace_id: Option<String>,
}

impl ProjectInfo {
    /// Build a fresh project info from the current working directory.
    pub fn from_env() -> Self {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            id: format!("proj_{}", uuid::Uuid::new_v4().simple()),
            worktree: cwd.clone(),
            directory: cwd,
            vcs: None,
            workspace_id: None,
        }
    }

    /// Update workspace id (used by v2 `Location.setWorkspace`).
    pub fn set_workspace(&mut self, workspace_id: Option<String>) {
        self.workspace_id = workspace_id;
    }
}

/// Session shape aligned with v2 spec (`protocols/http/session.md:113-130`).
/// All fields are serialized with `camelCase` aliases so the opencode TUI's
/// `Project.sync()` and `Session.sync()` can consume us directly.
#[derive(Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub slug: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    pub directory: String,
    pub title: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, rename = "parentID", serialize_with = "serialize_optional")]
    pub parent_id: Option<String>,
    #[serde(default, rename = "workspaceID", serialize_with = "serialize_optional")]
    pub workspace_id: Option<String>,
    #[serde(default, serialize_with = "serialize_path_as_string")]
    pub path: Option<PathInfo>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub summary: Option<SummaryInfo>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub cost: Option<f64>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub tokens: Option<TokensInfo>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub share: Option<ShareInfo>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub permission: Option<serde_json::Value>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub revert: Option<serde_json::Value>,
    /// Allowed optional fields per spec; represented as flat extras for
    /// MVP to avoid extending the schema every iteration.
    #[serde(flatten)]
    pub extras: HashMap<String, serde_json::Value>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub agent: Option<String>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub model: Option<ModelInfo>,
    #[serde(default)]
    pub time: TimeInfo,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PathInfo {
    pub cwd: String,
    pub root: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SummaryInfo {
    pub additions: i64,
    pub deletions: i64,
    pub files: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TokensInfo {
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,
    pub cache: TokensCacheInfo,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TokensCacheInfo {
    pub read: i64,
    pub write: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ShareInfo {
    pub url: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "id")]
    pub model_id: String,
    #[serde(default, serialize_with = "serialize_optional")]
    pub variant: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct TimeInfo {
    pub created: i64,
    pub updated: i64,
    #[serde(default, rename = "compacting", serialize_with = "serialize_optional")]
    pub compacting: Option<i64>,
    #[serde(default, rename = "archived", serialize_with = "serialize_optional")]
    pub archived: Option<i64>,
}

/// Minimal message shape — TUI primarily cares about `id`, `sessionID`,
/// `role`, `time`, `agent` for chat rendering and `parentID`/`finish`
/// for assistant turns.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct MessageInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub role: String,
    #[serde(default)]
    pub time: serde_json::Value,
    #[serde(default)]
    pub agent: String,
    #[serde(default, serialize_with = "serialize_optional")]
    pub model: Option<serde_json::Value>,
    #[serde(default, rename = "parentID", serialize_with = "serialize_optional")]
    pub parent_id: Option<String>,
    #[serde(default, rename = "tool", serialize_with = "serialize_optional")]
    pub tool: Option<serde_json::Value>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub finish: Option<String>,
    #[serde(default, rename = "providerID", serialize_with = "serialize_optional")]
    pub provider_id: Option<String>,
    #[serde(default, rename = "modelID", serialize_with = "serialize_optional")]
    pub model_id: Option<String>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub path: Option<serde_json::Value>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub cost: Option<f64>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub tokens: Option<serde_json::Value>,
    #[serde(default, serialize_with = "serialize_optional")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "summary")]
    pub summary_flag: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
}

/// We store the full opencode part payload as a `Value` blob and also
/// expose `id` / `sessionID` / `messageID` / `type` as top-level fields
/// for TUI list-rendering queries that don't want to deserialize the
/// payload (e.g. `GET /session/:id/messages` projections).
#[derive(Clone, Serialize, Deserialize)]
pub struct PartInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "type")]
    pub part_type: String,
    /// Full part payload (flattened into the JSON response alongside the
    /// typed fields above via `#[serde(flatten)]` on the response side).
    #[serde(skip)]
    pub data: serde_json::Value,
}

/// `Event.Durable` (schema/event.ts:33-37) — optional per-event durability
/// metadata carried on durable events.
#[derive(Clone, Serialize, Deserialize)]
pub struct EventDurable {
    #[serde(rename = "aggregateID")]
    pub aggregate_id: String,
    pub seq: u64,
    pub version: u32,
}

/// Internal event representation. The serialized wire shape follows the
/// opencode `EventSchema` flat union (schema/event.ts:54-61):
/// `{ id, metadata?, type, durable?, location?, data }`.
///
/// The `payload` and `directory` / `workspace` fields are kept as internal
/// accessors (used by the V1 serializer and session filtering) but are NOT
/// serialized directly — the custom [`Serialize`] impl hoists `id` and
/// `type` to the top level, renames `properties` → `data`, and nests
/// `directory` / `workspace` under `location`.
#[derive(Clone, Deserialize)]
pub struct GlobalEvent {
    /// Internal: the server directory the event originated from. Serialized
    /// under `location.directory` by the custom Serialize impl.
    pub directory: String,
    /// Internal: project id (unused in V2 flat shape, kept for V1 compat).
    pub project_id: Option<String>,
    /// Internal: workspace id. Serialized under `location.workspaceID`.
    pub workspace: Option<String>,
    /// Internal: id / type / properties triple.
    pub payload: EventPayload,
    /// Optional event metadata (schema/event.ts:39).
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    /// Optional durability info (schema/event.ts:33-37).
    #[serde(default)]
    pub durable: Option<EventDurable>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EventPayload {
    #[serde(rename = "id")]
    pub event_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub properties: serde_json::Value,
}

/// Custom serializer: emits the contract's flat `EventSchema` shape
/// (schema/event.ts:54-61). `id` and `type` are hoisted to top level,
/// `payload.properties` → `data`, `directory` / `workspace` → `location`,
/// plus optional `metadata` / `durable`. The legacy `payload` wrapper and
/// top-level `directory` / `project` / `workspace` are dropped.
impl Serialize for GlobalEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        // Count non-optional fields for a tight serialize_struct.
        let field_count = 3 // id, type, data
            + self.metadata.is_some() as usize
            + self.durable.is_some() as usize
            + (!self.directory.is_empty()) as usize;
        let mut state = serializer.serialize_struct("GlobalEvent", field_count)?;

        state.serialize_field("id", &self.payload.event_id)?;
        if let Some(metadata) = &self.metadata {
            state.serialize_field("metadata", metadata)?;
        }
        state.serialize_field("type", &self.payload.event_type)?;
        if let Some(durable) = &self.durable {
            state.serialize_field("durable", durable)?;
        }
        // location: { directory, workspaceID? } — included when directory is set.
        if !self.directory.is_empty() {
            let mut loc = serde_json::Map::new();
            loc.insert(
                "directory".to_string(),
                serde_json::Value::String(self.directory.clone()),
            );
            if let Some(ws) = &self.workspace {
                loc.insert(
                    "workspaceID".to_string(),
                    serde_json::Value::String(ws.clone()),
                );
            }
            state.serialize_field("location", &serde_json::Value::Object(loc))?;
        }
        state.serialize_field("data", &self.payload.properties)?;

        state.end()
    }
}

/// A live permission request raised when a tool needs approval (task LS-010).
///
/// Keyed by `id` in `AppState::permissions`. The `status` field transitions
/// `pending` -> `approved`/`denied` when the user replies via
/// `POST /permission/:id/reply`. The `time_created` timestamp is epoch
/// milliseconds to match the rest of the server's time fields.
#[derive(Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub tool: String,
    pub input: serde_json::Value,
    /// `"pending"`, `"approved"`, or `"denied"`.
    pub status: String,
    pub time_created: i64,
}

impl GlobalEvent {
    /// Construct a `GlobalEvent` with a fresh monotonic event id.
    pub fn new(
        directory: String,
        project_id: Option<String>,
        workspace: Option<String>,
        event_type: String,
        properties: serde_json::Value,
    ) -> Self {
        let event_id = {
            let mut g = NEXT_EVENT_ID.lock();
            let id = *g;
            *g = g.wrapping_add(1);
            format!("evt_{id}")
        };
        Self {
            directory,
            project_id,
            workspace,
            payload: EventPayload {
                event_id,
                event_type,
                properties,
            },
            metadata: None,
            durable: None,
        }
    }
}

/// Broadcast everything we generate. Subscribers serialize as JSON in `sse.rs`.
/// Also pushes into the bounded replay ring for `GET /api/session/:id/event`.
pub fn emit(state: &SharedState, event_type: &str, properties: serde_json::Value) {
    let directory = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let (project_id, workspace) = {
        let project = state.project.read();
        (Some(project.id.clone()), project.workspace_id.clone())
    };
    let event = GlobalEvent::new(
        directory,
        project_id,
        workspace,
        event_type.to_string(),
        properties,
    );

    // Push into bounded ring before broadcast so subscribers can drain
    // `event_buffer` after the live notification if they want to.
    push_event_buffer(state, event.clone());

    // Write-through to the durable store (if enabled).
    if let Some(store) = &state.store {
        store.push_event(&event);
    }

    let _ = state.event_tx.send(event);
}

/// Bound the replay ring at `EVENT_BUFFER_CAP`. Older events are evicted
/// from the front; callers can adjust via `truncate_event_buffer` if
/// a different cap becomes desirable.
pub fn push_event_buffer(state: &SharedState, event: GlobalEvent) {
    let mut buf = state.event_buffer.write();
    if buf.len() >= EVENT_BUFFER_CAP {
        buf.pop_front();
    }
    buf.push_back(event);
}

/// Snapshot the current buffer — used by `GET /api/session/:id/event`.
pub fn snapshot_replay(state: &SharedState, after: Option<&str>) -> Vec<GlobalEvent> {
    let buf = state.event_buffer.read();
    if let Some(after) = after {
        // walk backwards from the tail looking for the matching event id
        let mut idx = None;
        for (i, ev) in buf.iter().enumerate().rev() {
            if ev.payload.event_id == after {
                idx = Some(i);
                break;
            }
        }
        match idx {
            Some(i) => buf.iter().skip(i + 1).cloned().collect(),
            None => buf.iter().cloned().collect(),
        }
    } else {
        buf.iter().cloned().collect()
    }
}

pub fn new_session_id() -> String {
    format!("sess_{}", uuid::Uuid::new_v4().simple())
}

static LAST_ID_TS: parking_lot::Mutex<(u64, u16)> = parking_lot::Mutex::new((0, 0));
const ID_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn ascending_id(prefix: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let (ts, counter) = {
        let mut guard = LAST_ID_TS.lock();
        if now == guard.0 {
            guard.1 = guard.1.wrapping_add(1);
            (guard.0, guard.1)
        } else {
            *guard = (now, 1);
            (now, 1)
        }
    };
    let current = ts * 0x1000 + counter as u64;
    let time_hex = format!("{current:012x}");
    let uuid = uuid::Uuid::new_v4();
    let rand_bytes = uuid.as_bytes();
    let suffix: String = (0..14)
        .map(|i| ID_CHARS[(rand_bytes[i % 16] % 62) as usize] as char)
        .collect();
    format!("{prefix}{time_hex}{suffix}")
}

pub fn new_message_id() -> String {
    ascending_id("msg_")
}

pub fn new_part_id() -> String {
    ascending_id("prt_")
}

pub fn new_permission_id() -> String {
    format!("perm_{}", uuid::Uuid::new_v4().simple())
}

// ===========================================================================
// Credential store (schema-credential.ts, group-credential.ts)
// Backs `PATCH/DELETE /api/credential/:credentialID`.
// ===========================================================================

/// `Credential.ID` is `"cred_" + ascending()` (schema-credential.ts).
pub fn new_credential_id() -> String {
    static GEN: parking_lot::Mutex<u64> = parking_lot::Mutex::new(0);
    let mut g = GEN.lock();
    let cur = *g;
    *g = g.wrapping_add(1);
    format!("cred_{cur}")
}

/// `Credential.Value` union, tagged by `type` (`oauth` | `key`)
/// (schema-credential.ts).
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CredentialValue {
    /// `Credential.OAuth` — integration OAuth token bundle.
    #[serde(rename = "oauth")]
    Oauth {
        #[serde(rename = "methodID")]
        method_id: String,
        refresh: String,
        access: String,
        expires: u64,
        #[serde(default, serialize_with = "serialize_optional")]
        metadata: Option<serde_json::Value>,
    },
    /// `Credential.Key` — raw API key.
    #[serde(rename = "key")]
    Key {
        key: String,
        #[serde(default, serialize_with = "serialize_optional")]
        metadata: Option<serde_json::Value>,
    },
}

/// One stored credential keyed by `cred_*` id. The `label` is the only field
/// mutated via `PATCH /api/credential/:credentialID` (payload `{ label }`);
/// `value` carries the secret material set during an auth flow.
#[derive(Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    pub label: String,
    #[serde(default, serialize_with = "serialize_optional")]
    pub value: Option<CredentialValue>,
}

// ===========================================================================
// PTY registry + connect tickets (schema-pty.ts, group-pty.ts)
// Backs `/api/pty[/:ptyID]` CRUD and the WebSocket connect flow.
// ===========================================================================

/// `Pty.ID` is `"pty_" + ascending()` (schema-pty.ts).
pub fn new_pty_id() -> String {
    static GEN: parking_lot::Mutex<u64> = parking_lot::Mutex::new(0);
    let mut g = GEN.lock();
    let cur = *g;
    *g = g.wrapping_add(1);
    format!("pty_{cur}")
}

/// `Pty.Info` shape (schema-pty.ts). `status` is `"running"` or `"exited"`;
/// `exit_code` is present only after the child exits.
#[derive(Clone, Serialize, Deserialize)]
pub struct PtyInfo {
    pub id: String,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// `"running"` | `"exited"`.
    pub status: String,
    pub pid: u32,
    #[serde(default, rename = "exitCode", serialize_with = "serialize_optional")]
    pub exit_code: Option<u32>,
}

/// Live PTY handle held in the registry. The `portable_pty` master is wrapped
/// in a `Mutex` (it is `Send` but not `Sync`) so the whole handle is
/// `Send + Sync` and can live behind the registry's `RwLock`. The child is
/// retained so handlers can read the pid, kill, and await the exit code.
/// `info` carries the mutable `Pty.Info` metadata returned by the routes.
pub struct PtyHandle {
    pub master: parking_lot::Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    pub child: parking_lot::Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    pub info: parking_lot::RwLock<PtyInfo>,
}

/// A single-use, short-lived ticket granting one WebSocket connect to a PTY
/// (group-pty.ts: `POST /api/pty/:ptyID/connect-token` →
/// `GET .../connect?ticket=`). `created_at` (epoch millis) lets a future
/// reaper evict stale tickets; `ticket` strings are opaque (`tkt_…`).
#[derive(Clone)]
pub struct PtyTicket {
    pub pty_id: String,
    pub created_at: i64,
}

/// Mint a fresh opaque ticket string.
pub fn new_pty_ticket() -> String {
    format!("tkt_{}", uuid::Uuid::new_v4().simple())
}

/// Allocate a fresh `RunCancellation` and remember it under `session_id`
/// so the abort route can find it (task P1.11).
pub fn begin_run(state: &SharedState, session_id: &str) -> RunCancellation {
    let cancellation = RunCancellation::new({
        static GEN: parking_lot::Mutex<u64> = parking_lot::Mutex::new(0);
        let mut g = GEN.lock();
        let cur = *g;
        *g = g.wrapping_add(1);
        cur
    });
    if let Some(previous) = state
        .abort_tokens
        .write()
        .insert(session_id.to_string(), cancellation.clone())
    {
        previous.cancel();
    }
    cancellation
}

/// Remove a run only if it is still the generation that just finished.
/// A cancelled older task must not remove a replacement run for the same
/// session.
pub fn end_run(state: &SharedState, session_id: &str, generation: u64) {
    let mut runs = state.abort_tokens.write();
    if runs
        .get(session_id)
        .is_some_and(|run| run.generation() == generation)
    {
        runs.remove(session_id);
    }
}

/// Lookup the cancellation token for an active run, if any. Returns
/// `None` for sessions that are not currently running.
pub fn lookup_run(state: &SharedState, session_id: &str) -> Option<RunCancellation> {
    state.abort_tokens.read().get(session_id).cloned()
}

/// Write-through helper: upsert a session into the store (if enabled).
/// No-op when `store` is `None` (tests).
pub fn persist_session(state: &SharedState, session: &SessionInfo) {
    if let Some(store) = &state.store {
        store.save_session(session);
    }
}

/// Write-through helper: delete a session from the store (if enabled).
pub fn persist_session_delete(state: &SharedState, id: &str) {
    if let Some(store) = &state.store {
        store.delete_session(id);
    }
}

/// Write-through helper: persist the current message list for a session.
/// Reads the in-memory map and forwards a snapshot to the store.
pub fn persist_messages(state: &SharedState, session_id: &str) {
    if let Some(store) = &state.store {
        let messages = state
            .messages
            .read()
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        store.save_messages(session_id, &messages);
    }
}

/// Write-through helper: persist the current part list for a message.
pub fn persist_parts(state: &SharedState, message_id: &str) {
    if let Some(store) = &state.store {
        let parts = state
            .parts
            .read()
            .get(message_id)
            .cloned()
            .unwrap_or_default();
        store.save_parts(message_id, &parts);
    }
}

/// Write-through helper: cascade-delete a session's messages and parts from
/// the store (if enabled). Called from `delete_session`.
pub fn persist_session_cascade(state: &SharedState, session_id: &str, message_ids: &[String]) {
    if let Some(store) = &state.store {
        store.delete_messages(session_id);
        for mid in message_ids {
            store.delete_parts(mid);
        }
    }
}

/// Load-on-startup: populate in-memory maps from the store. No-op when
/// `store` is `None` (tests).
pub fn load_from_store(state: &SharedState) {
    let Some(store) = &state.store else {
        return;
    };
    *state.sessions.write() = store.load_sessions();
    *state.messages.write() = store.load_messages();
    *state.parts.write() = store.load_parts();
    *state.event_buffer.write() = store.load_events();
    let v2_events = store.load_v2_session_events();
    let v2_next_seq = v2_events
        .iter()
        .map(|(session_id, events)| {
            let next = events
                .iter()
                .filter_map(|event| event.durable.as_ref().map(|durable| durable.seq))
                .max()
                .unwrap_or(0)
                + 1;
            (session_id.clone(), next)
        })
        .collect();
    *state.v2_session_events.write() = v2_events;
    *state.v2_next_seq.write() = v2_next_seq;
}

/// Reload config from disk and update in-memory state. Called by the SIGHUP
/// handler (Unix) and `POST /config/reload` handler (cross-platform).
/// Returns the number of providers in the reloaded config, or an error.
/// On error, the previous config is preserved.
pub fn reload_config_from_disk(state: &SharedState) -> Result<usize, String> {
    let cfg = config::load_full_config("loom").map_err(|e| format!("{e}"))?;
    let count = cfg.providers.len();

    if let Some(default) = cfg.default_provider.as_ref() {
        let mut config = state.config.write();
        config.provider = Some(serde_json::json!(default));
    }

    Ok(count)
}

/// Build an isolated state for tests and in-process callers.
pub fn new_state() -> SharedState {
    new_state_inner(false)
}

/// Build production server state with config persistence enabled.
pub fn new_server_state() -> SharedState {
    new_state_inner(true)
}

/// Build production server state with an in-memory store wired in for
/// write-through persistence and load-on-startup. Exposed so future code
/// can swap in a SQLite/file-backed store by passing a different
/// `Arc<dyn StoreTrait + Send + Sync>`.
pub fn new_server_state_with_store() -> SharedState {
    let store: Arc<dyn StoreTrait + Send + Sync> = Arc::new(InMemoryStore::new());
    new_state_with_store(true, Some(store))
}

fn new_state_inner(persist_config: bool) -> SharedState {
    new_state_with_store(persist_config, None)
}

fn new_state_with_store(
    persist_config: bool,
    store: Option<Arc<dyn StoreTrait + Send + Sync>>,
) -> SharedState {
    let (event_tx, _) = broadcast::channel(1024);
    let (v2_event_tx, _) = broadcast::channel(1024);
    let v2_file_log = if persist_config {
        crate::v2_event::V2FileLog::open(config::home::loom_home().join("server").join("v2-events"))
            .map(Arc::new)
            .map_err(|error| tracing::error!(%error, "failed to initialize v2 durable log"))
            .ok()
    } else {
        None
    };
    let state = Arc::new(AppState {
        acp_hub: Arc::new(crate::acp_hub::AcpHub::default()),
        sessions: RwLock::new(HashMap::new()),
        messages: RwLock::new(HashMap::new()),
        parts: RwLock::new(HashMap::new()),
        active_text: RwLock::new(HashMap::new()),
        active_reasoning: RwLock::new(HashMap::new()),
        abort_tokens: RwLock::new(HashMap::new()),
        event_tx,
        event_buffer: RwLock::new(VecDeque::with_capacity(EVENT_BUFFER_CAP)),
        v2_event_tx,
        v2_session_events: RwLock::new(HashMap::new()),
        v2_next_seq: RwLock::new(HashMap::new()),
        v2_reasoning_ids: RwLock::new(HashMap::new()),
        v2_started_tool_calls: RwLock::new(HashSet::new()),
        v2_terminal_tool_calls: RwLock::new(HashSet::new()),
        session_contexts: RwLock::new(HashMap::new()),
        staged_reverts: RwLock::new(HashMap::new()),
        v2_file_log,
        v2_publish_lock: Mutex::new(()),
        config: RwLock::new(ConfigInfo::default()),
        persist_config,
        project: RwLock::new(ProjectInfo::from_env()),
        permissions: RwLock::new(HashMap::new()),
        store,
        credentials: RwLock::new(HashMap::new()),
        ptys: RwLock::new(HashMap::new()),
        pty_tickets: RwLock::new(HashMap::new()),
    });
    if state.store.is_some() {
        load_from_store(&state);
    }
    crate::v2_event::load_file_log(&state);
    state
}

/// Default `SessionInfo` factory — used by handlers that create new
/// sessions. Pulls defaults from `AppState::project` so the v2 envelope
/// fields stay consistent.
pub fn make_session(state: &SharedState, agent: Option<String>) -> SessionInfo {
    let project = state.project.read().clone();
    let now = chrono::Utc::now().timestamp_millis();
    let id = new_session_id();
    SessionInfo {
        id: id.clone(),
        slug: id.clone(),
        project_id: project.id.clone(),
        directory: project.directory.clone(),
        title: "New Session".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        parent_id: None,
        workspace_id: project.workspace_id.clone(),
        path: Some(PathInfo {
            cwd: project.directory.clone(),
            root: project.worktree.clone(),
        }),
        summary: None,
        cost: Some(0.0),
        tokens: Some(TokensInfo {
            input: 0,
            output: 0,
            reasoning: 0,
            cache: TokensCacheInfo { read: 0, write: 0 },
        }),
        share: None,
        permission: None,
        revert: None,
        extras: HashMap::new(),
        agent,
        model: None,
        time: TimeInfo {
            created: now,
            updated: now,
            compacting: None,
            archived: None,
        },
        metadata: serde_json::json!({}),
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::{begin_run, end_run, lookup_run, new_state};

    #[test]
    fn replacement_run_cancels_previous_and_generation_cleanup_is_safe() {
        let state = new_state();
        let first = begin_run(&state, "sess_test");
        let second = begin_run(&state, "sess_test");

        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        end_run(&state, "sess_test", first.generation());
        assert_eq!(
            lookup_run(&state, "sess_test").map(|run| run.generation()),
            Some(second.generation())
        );

        end_run(&state, "sess_test", second.generation());
        assert!(lookup_run(&state, "sess_test").is_none());
    }
}

#[cfg(test)]
mod optional_field_serialization {
    //! Regression tests for the `U.has` TUI crash.
    //!
    //! The opencode/openchamber TUI does `Object.has(info, 'parentID')`,
    //! `info.tool.has(...)`, etc. on every reactive re-render. If those
    //! fields are *omitted* (rather than serialized as JSON `null`), the
    //! JS lookup target becomes `undefined` and the first keypress crashes
    //! the TUI with `undefined is not an object (evaluating 'U.has')`.
    //!
    //! These tests pin the contract: every `Option<T>` on `SessionInfo`,
    //! `MessageInfo`, and friends is serialized as a stable field whose
    //! value is either the inner `T` or `null`, never absent.

    use super::*;

    fn assert_field_present(value: &serde_json::Value, key: &str) {
        assert!(
            value.get(key).is_some(),
            "field `{key}` must be present in serialized JSON (got {})",
            value,
        );
    }

    fn assert_field_null(value: &serde_json::Value, key: &str) {
        assert_field_present(value, key);
        assert_eq!(
            value.get(key),
            Some(&serde_json::Value::Null),
            "field `{key}` must be serialized as JSON null when the Rust \
             `Option<T>` is `None` (got {})",
            value,
        );
    }

    #[test]
    fn session_info_none_option_fields_serialize_as_null() {
        let state = new_state();
        let mut session = make_session(&state, None);
        session.parent_id = None;
        session.workspace_id = None;
        session.summary = None;
        session.share = None;
        session.permission = None;
        session.revert = None;
        session.agent = None;
        session.model = None;

        let json = serde_json::to_value(&session).expect("serialize session");

        for key in [
            "parentID",
            "workspaceID",
            "summary",
            "share",
            "permission",
            "revert",
            "agent",
            "model",
        ] {
            assert_field_null(&json, key);
        }
        // These are explicitly Some — keep them as a positive control so a
        // blanket-all-null mistake doesn't slip past.
        assert_field_present(&json, "id");
        assert!(json["id"].is_string());
    }

    #[test]
    fn message_info_none_option_fields_serialize_as_null() {
        let info = MessageInfo {
            id: "msg_x".into(),
            session_id: "sess_x".into(),
            role: "assistant".into(),
            time: serde_json::json!({}),
            agent: "build".into(),
            model: None,
            parent_id: None,
            tool: None,
            finish: None,
            provider_id: None,
            model_id: None,
            path: None,
            cost: None,
            tokens: None,
            mode: None,
            ..Default::default()
        };

        let json = serde_json::to_value(&info).expect("serialize message");

        for key in [
            "model",
            "parentID",
            "tool",
            "finish",
            "providerID",
            "modelID",
            "path",
            "cost",
            "tokens",
            "mode",
        ] {
            assert_field_null(&json, key);
        }
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["sessionID"], "sess_x");
    }

    #[test]
    fn message_info_some_option_fields_serialize_as_inner_value() {
        let info = MessageInfo {
            id: "msg_x".into(),
            session_id: "sess_x".into(),
            role: "user".into(),
            time: serde_json::json!({}),
            agent: "build".into(),
            model: Some(serde_json::json!({"providerID": "p", "modelID": "m"})),
            parent_id: Some("msg_parent".into()),
            tool: Some(serde_json::json!({"name": "bash"})),
            finish: Some("stop".into()),
            provider_id: Some("p".into()),
            model_id: Some("m".into()),
            path: Some(serde_json::json!({"cwd": "/tmp"})),
            cost: Some(0.42),
            tokens: Some(serde_json::json!({"input": 1, "output": 2})),
            mode: Some("build".into()),
            ..Default::default()
        };

        let json = serde_json::to_value(&info).expect("serialize message");

        // The `Some` branch must keep using the inner T — not fall back to
        // `serialize_none`. Verifies the helper is wired correctly.
        assert_eq!(json["parentID"], "msg_parent");
        assert_eq!(json["finish"], "stop");
        assert_eq!(json["providerID"], "p");
        assert_eq!(json["modelID"], "m");
        assert_eq!(json["cost"], 0.42);
        assert_eq!(json["mode"], "build");
        assert_eq!(json["model"]["providerID"], "p");
        assert_eq!(json["tool"]["name"], "bash");
    }

    #[test]
    fn time_info_optional_subfields_serialize_as_null() {
        let info = TimeInfo {
            created: 1,
            updated: 2,
            compacting: None,
            archived: None,
        };

        let json = serde_json::to_value(&info).expect("serialize time");
        assert_field_null(&json, "compacting");
        assert_field_null(&json, "archived");
        assert_eq!(json["created"], 1);
        assert_eq!(json["updated"], 2);
    }

    #[test]
    fn pty_info_exit_code_absent_while_running_serializes_as_null() {
        let info = PtyInfo {
            id: "pty_x".into(),
            title: "t".into(),
            command: "bash".into(),
            args: vec![],
            cwd: "/".into(),
            status: "running".into(),
            pid: 1234,
            exit_code: None,
        };

        let json = serde_json::to_value(&info).expect("serialize pty");
        assert_field_null(&json, "exitCode");
        assert_eq!(json["status"], "running");
    }

    #[test]
    fn deserialization_tolerates_missing_option_fields() {
        // Round-trip guarantee: an external client that omits (instead of
        // nullifying) optional fields must still parse back into a usable
        // Rust value. The `default` attribute on every `Option<T>` field
        // makes this work.
        let bare: serde_json::Value = serde_json::json!({
            "id": "sess_x",
            "slug": "sess_x",
            "projectID": "proj_x",
            "directory": "/tmp",
            "title": "t",
        });

        let parsed: SessionInfo = serde_json::from_value(bare).expect("deserialize bare session");
        assert_eq!(parsed.id, "sess_x");
        assert!(parsed.parent_id.is_none());
        assert!(parsed.summary.is_none());
        assert!(parsed.model.is_none());
    }
}
