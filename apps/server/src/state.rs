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
//! In-memory only — restart drops everything.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::broadcast;
use tool_core::active_operation::RunCancellation;

/// `sess_*` counter — used by the v2 replay buffer to assign monotonic
/// event ids without depending on `uuid` ordering. Random u64 is good
/// enough; we only need uniqueness within a single SSE session per the
/// spec (`protocols/sse-events.md:75-91`).
static NEXT_EVENT_ID: parking_lot::Mutex<u64> = parking_lot::Mutex::new(1);

/// Shared handle handed to every route via `axum::extract::State`.
pub type SharedState = Arc<AppState>;

/// All mutable state lives behind `RwLock` so handlers can take it
/// without `async` lock contention on the hot path. The SSE broadcast
/// sender is unbounded enough for our 1024-message buffer, and event
/// replay uses a separate bounded ring buffer.
pub struct AppState {
    /// `sess_*` → session metadata (matches opencode's v2 `Session.Info` shape).
    pub sessions: RwLock<HashMap<String, SessionInfo>>,
    /// `sess_*` → ordered list of `MessageInfo` (user + assistant turns).
    pub messages: RwLock<HashMap<String, Vec<MessageInfo>>>,
    /// `msg_*` → ordered list of `PartInfo` (text / tool / reasoning).
    ///
    /// Keyed by message id, not session id, because part lookups in the
    /// translator happen per-message during streaming.
    pub parts: RwLock<HashMap<String, Vec<PartInfo>>>,
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
    /// Global config blob (task P2.23 + P0.2). Read on `GET /config` and
    /// `GET /global/config`; written through `PATCH /global/config`.
    pub config: RwLock<ConfigInfo>,
    /// Persist global config updates when running the production binary.
    pub persist_config: bool,
    /// Project metadata stored on first `/project` request (v2 spec
    /// injects `directory + project + workspace` into the SSE envelope).
    pub project: RwLock<ProjectInfo>,
}

const EVENT_BUFFER_CAP: usize = 512;

/// All mutable config lives here. v2 spec (§4.1) defines a `ConfigInfo`
/// shape; we keep our MVP scaffold small and add fields as handlers need them.
#[derive(Clone, Default, Serialize)]
pub struct ConfigInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<serde_json::Value>,
    /// Anything extra callers want to stash; opt-in via `_meta`.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub extra: serde_json::Value,
}

/// Project metadata required by the v2 envelope (`GlobalEventSchema`).
#[derive(Clone, Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub worktree: String,
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcs: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
#[derive(Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub slug: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    pub directory: String,
    pub title: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "parentID")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "workspaceID")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SummaryInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokensInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<ShareInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert: Option<serde_json::Value>,
    /// Allowed optional fields per spec; represented as flat extras for
    /// MVP to avoid extending the schema every iteration.
    #[serde(flatten)]
    pub extras: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelInfo>,
    pub time: TimeInfo,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

#[derive(Clone, Serialize)]
pub struct PathInfo {
    pub cwd: String,
    pub root: String,
}

#[derive(Clone, Serialize)]
pub struct SummaryInfo {
    pub additions: i64,
    pub deletions: i64,
    pub files: i64,
}

#[derive(Clone, Serialize)]
pub struct TokensInfo {
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,
    pub cache: TokensCacheInfo,
}

#[derive(Clone, Serialize)]
pub struct TokensCacheInfo {
    pub read: i64,
    pub write: i64,
}

#[derive(Clone, Serialize)]
pub struct ShareInfo {
    pub url: String,
}

#[derive(Clone, Serialize)]
pub struct ModelInfo {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct TimeInfo {
    pub created: i64,
    pub updated: i64,
    #[serde(skip_serializing_if = "Option::is_none", rename = "compacting")]
    pub compacting: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "archived")]
    pub archived: Option<i64>,
}

/// Minimal message shape — TUI primarily cares about `id`, `sessionID`,
/// `role`, `time`, `agent` for chat rendering and `parentID`/`finish`
/// for assistant turns.
#[derive(Clone, Serialize)]
pub struct MessageInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub role: String,
    pub time: serde_json::Value,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "parentID")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "tool")]
    pub tool: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
    #[serde(rename = "providerID", skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(rename = "modelID", skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// We store the full opencode part payload as a `Value` blob and also
/// expose `id` / `sessionID` / `messageID` / `type` as top-level fields
/// for TUI list-rendering queries that don't want to deserialize the
/// payload (e.g. `GET /session/:id/messages` projections).
#[derive(Clone, Serialize)]
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

/// v2 envelope (`protocols/http/global.md:263-279`). Required
/// `directory`, optional `project?` / `workspace?`. The `payload` union
/// is rendered with `#[serde(flatten)]` on `EventPayload` so the wire
/// shape stays a flat object.
#[derive(Clone, Serialize)]
pub struct GlobalEvent {
    pub directory: String,
    #[serde(rename = "project", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub payload: EventPayload,
}

#[derive(Clone, Serialize)]
pub struct EventPayload {
    #[serde(rename = "id")]
    pub event_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub properties: serde_json::Value,
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

pub fn new_message_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4().simple())
}

pub fn new_part_id() -> String {
    format!("prt_{}", uuid::Uuid::new_v4().simple())
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

/// Build an isolated state for tests and in-process callers.
pub fn new_state() -> SharedState {
    new_state_inner(false)
}

/// Build production server state with config persistence enabled.
pub fn new_server_state() -> SharedState {
    new_state_inner(true)
}

fn new_state_inner(persist_config: bool) -> SharedState {
    let (event_tx, _) = broadcast::channel(1024);
    Arc::new(AppState {
        sessions: RwLock::new(HashMap::new()),
        messages: RwLock::new(HashMap::new()),
        parts: RwLock::new(HashMap::new()),
        abort_tokens: RwLock::new(HashMap::new()),
        event_tx,
        event_buffer: RwLock::new(VecDeque::with_capacity(EVENT_BUFFER_CAP)),
        config: RwLock::new(ConfigInfo::default()),
        persist_config,
        project: RwLock::new(ProjectInfo::from_env()),
    })
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
