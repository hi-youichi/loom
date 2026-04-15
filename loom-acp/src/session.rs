//! Session state: session_id mapping, cancel flag, working directory
//!
//! Each ACP `session/new` corresponds to one [`SessionEntry`], stored by [`SessionStore`] keyed by session_id.
//! Protocol details are in [`crate::protocol`].
//!
//! ## NewSessionRequest -> SessionStore
//!
//! - **session_id**: Generated uniquely by Agent in new_session (e.g. `session-{nanos}` or UUID); all later prompt/cancel/load use this ID.
//! - **thread_id**: Same as Loom's `RunOptions::thread_id`, usually the string form of session_id for checkpointer and multi-turn consistency.
//! - **working_directory**: From `NewSessionRequest::working_directory` (protocol requires **absolute path**), maps to `RunOptions::working_folder`; if absent the caller may use process cwd or a temp dir.
//! - **mcp_servers**: Connect in new_session; disconnect when session is "closed" or process exits; this module only stores session metadata; MCP connection table can live in Agent or a separate layer.
//!
//! ## Cancel semantics (session/cancel)
//!
//! - **cancelled**: Whether the session has been cancelled by the Client via `session/cancel`. On cancel call [`SessionStore::set_cancelled`]; the prompt path should **periodically** check [`SessionStore::is_cancelled`] and exit with StopReason::Cancelled when true. Any pending request_permission will get Cancelled from the Client.
//!
//! When integrated with ACP, session_id can use `agent_client_protocol::SessionId`; this module's [`SessionId`] is a placeholder type for unit tests without the ACP dependency.

use loom::RunCancellation;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// Unique session identifier.
///
/// Without ACP this type (inner `String`) is used; at the boundary it can be converted to/from
/// `agent_client_protocol::SessionId`, or the protocol type can be used as the key directly.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SessionId(pub String);

impl SessionId {
    /// Create a SessionId from a string.
    #[inline]
    pub fn new(s: impl Into<String>) -> Self {
        SessionId(s.into())
    }

    /// Return the underlying string.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Per-session configuration (e.g. model, max_tokens). Set via `session/set_config_option`.
#[derive(Clone, Debug, Default)]
pub struct SessionConfig {
    /// LLM model id for this session (e.g. "gpt-4o", "gpt-4o-mini"). When set, overrides env at prompt time.
    pub model: Option<String>,
    /// Current agent/mode id for this session (e.g. "ask", "default", "dev"). Maps to ACP session mode.
    pub current_agent: String,
}

/// Metadata and cancel flag for a single session.
///
/// Written by [`SessionStore::create`], read by [`SessionStore::get`].
/// Prompt handling uses `thread_id` and `working_directory` to build [`loom::RunOptions`]
/// and [`SessionStore::is_cancelled`] to decide whether to abort with Cancelled.
#[derive(Debug)]
pub struct SessionEntry {
    /// Thread/session id used by Loom; 1:1 with ACP session_id.
    pub thread_id: String,
    /// Working directory for this session (from NewSessionRequest); None lets the caller choose a default.
    pub working_directory: Option<PathBuf>,
    /// Whether this session has been cancelled via session/cancel; should be checked periodically in the prompt path.
    pub cancelled: AtomicBool,
    /// Session-level config (model, etc.); updated by set_session_config_option.
    pub session_config: SessionConfig,
    /// Shared cancellation state for the current turn.
    pub cancellation: Arc<SessionCancellationState>,
}

#[derive(Debug, Default)]
pub struct SessionCancellationState {
    pub current_generation: AtomicU64,
    pub current_turn: std::sync::RwLock<Option<Arc<RunningTurn>>>,
}

#[derive(Debug)]
pub struct RunningTurn {
    pub generation: u64,
    pub cancellation: RunCancellation,
}

/// In-memory session table: session_id -> [`SessionEntry`].
///
/// Concurrent reads and single-writer (RwLock); cancel flag is atomic so it can be checked without the lock.
/// Sessions live for the process; no persistence after exit.
#[derive(Debug, Default)]
pub struct SessionStore {
    inner: std::sync::RwLock<std::collections::HashMap<SessionId, SessionEntry>>,
}

impl SessionStore {
    /// Create an empty session store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new session and return its [`SessionId`].
    ///
    /// Corresponds to ACP `session/new`: Agent generates a unique session_id and adds an entry.
    /// `working_directory` comes from `NewSessionRequest::working_directory` (protocol requires absolute path);
    /// if not provided, pass `None`; prompt handling may use process cwd or another default.
    pub fn create(&self, working_directory: Option<PathBuf>) -> SessionId {
        let session_id = SessionId(format!("session-{}", Uuid::new_v4()));
        self.create_with_id(session_id.clone(), working_directory, session_id.0.clone());
        session_id
    }

    /// Create a session with a specific session_id and thread_id.
    ///
    /// Used by `session/load` when loading an existing session.
    /// If a session with the same id already exists, returns the existing entry.
    pub fn create_with_id(
        &self,
        session_id: SessionId,
        working_directory: Option<PathBuf>,
        thread_id: String,
    ) -> SessionEntry {
        let mut guard = self.inner.write().unwrap();
        if let Some(existing) = guard.get(&session_id) {
            return existing.clone();
        }
        let entry = SessionEntry {
            thread_id,
            working_directory,
            cancelled: AtomicBool::new(false),
            session_config: SessionConfig::default(),
            cancellation: Arc::new(SessionCancellationState::default()),
        };
        guard.insert(session_id.clone(), entry.clone());
        entry
    }

    /// Look up a session by session_id; returns `None` if not found.
    pub fn get(&self, session_id: &SessionId) -> Option<SessionEntry> {
        self.inner.read().unwrap().get(session_id).cloned()
    }

    /// Mark the given session as cancelled (call when receiving `session/cancel`).
    ///
    /// No-op if session_id is not in the store.
    pub fn set_cancelled(&self, session_id: SessionId) {
        self.cancel_current_generation(&session_id);
    }

    /// Begin a new prompt generation and return a fresh runtime cancellation handle.
    pub fn begin_prompt(&self, session_id: &SessionId) -> Option<RunCancellation> {
        if let Some(entry) = self.inner.read().unwrap().get(session_id) {
            let generation = entry
                .cancellation
                .current_generation
                .fetch_add(1, Ordering::SeqCst)
                + 1;
            let cancellation = RunCancellation::new(generation);
            let turn = Arc::new(RunningTurn {
                generation,
                cancellation: cancellation.clone(),
            });
            if let Ok(mut current_turn) = entry.cancellation.current_turn.write() {
                *current_turn = Some(turn);
            }
            entry.cancelled.store(false, Ordering::SeqCst);
            return Some(cancellation);
        }
        None
    }

    /// Mark the current generation as cancelled and trigger its runtime token.
    pub fn cancel_current_generation(&self, session_id: &SessionId) {
        if let Some(entry) = self.inner.read().unwrap().get(session_id) {
            entry.cancelled.store(true, Ordering::SeqCst);
            if let Ok(current_turn) = entry.cancellation.current_turn.read() {
                if let Some(turn) = current_turn.as_ref() {
                    turn.cancellation.cancel();
                }
            }
        }
    }

    /// Clear the current running turn when the prompt owner finishes.
    pub fn finish_prompt(&self, session_id: &SessionId, generation: u64) {
        if let Some(entry) = self.inner.read().unwrap().get(session_id) {
            if let Ok(mut current_turn) = entry.cancellation.current_turn.write() {
                let should_clear = current_turn
                    .as_ref()
                    .map(|turn| turn.generation == generation)
                    .unwrap_or(false);
                if should_clear {
                    *current_turn = None;
                }
            }
        }
    }

    /// Return whether this session has been cancelled.
    ///
    /// Returns `false` if session_id is not in the store.
    pub fn is_cancelled(&self, session_id: &SessionId) -> bool {
        self.inner
            .read()
            .unwrap()
            .get(session_id)
            .map(|e| e.cancelled.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// Update session config for the given session. No-op if session_id is not in the store.
    pub fn update_session_config<F>(&self, session_id: &SessionId, f: F)
    where
        F: FnOnce(&mut SessionConfig),
    {
        if let Ok(mut guard) = self.inner.write() {
            if let Some(entry) = guard.get_mut(session_id) {
                f(&mut entry.session_config);
            }
        }
    }
}

impl Clone for SessionEntry {
    fn clone(&self) -> Self {
        SessionEntry {
            thread_id: self.thread_id.clone(),
            working_directory: self.working_directory.clone(),
            cancelled: AtomicBool::new(self.cancelled.load(Ordering::SeqCst)),
            session_config: self.session_config.clone(),
            cancellation: Arc::clone(&self.cancellation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_config_model_updated_by_update_session_config() {
        let store = SessionStore::new();
        let id = store.create(None);
        assert!(store.get(&id).unwrap().session_config.model.is_none());

        store.update_session_config(&id, |c| c.model = Some("gpt-4o".to_string()));
        assert_eq!(
            store.get(&id).unwrap().session_config.model.as_deref(),
            Some("gpt-4o")
        );
    }

    #[test]
    fn begin_prompt_cancel_and_finish_manage_current_turn() {
        let store = SessionStore::new();
        let id = store.create(None);

        let cancellation = store.begin_prompt(&id).expect("begin prompt");
        assert!(!cancellation.token().is_cancelled());
        assert!(!store.is_cancelled(&id));

        store.cancel_current_generation(&id);
        assert!(store.is_cancelled(&id));
        assert!(cancellation.token().is_cancelled());

        store.finish_prompt(&id, cancellation.generation());
        let entry = store.get(&id).expect("session entry");
        assert!(entry
            .cancellation
            .current_turn
            .read()
            .expect("read current turn")
            .is_none());
    }
}
