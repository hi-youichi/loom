//! Skill write-origin provenance — task-local context for distinguishing
//! background review writes from foreground user-directed writes.
//!
//! The curator only consolidates/prunes skills it autonomously created via
//! background self-improvement review. Skills a user asks a foreground
//! agent to write belong to the user and must never be auto-curated.
//!
//! # Storage
//!
//! Migrated from `thread_local!` to `tokio::task_local!` so the origin
//! survives across `.await` points when the runtime reschedules a task
//! to a different worker thread. `task_local!` propagates through the
//! Future tree (including across `.await`), matching the semantics of
//! Python's `contextvars.ContextVar` that Hermes uses.
//!
//! # Usage
//!
//! ```ignore
//! use skill::provenance::{WriteOrigin, with_write_origin};
//!
//! // Wrap an async block in background-review context:
//! let result = with_write_origin(WriteOrigin::BackgroundReview, async {
//!     // ... tool runs here — WriteOrigin::current() returns BackgroundReview
//!     //   across any internal .await points ...
//! }).await;
//!
//! // From any code inside the scope:
//! let origin = WriteOrigin::current();
//! if matches!(origin, WriteOrigin::BackgroundReview) {
//!     store.mark_agent_created(name);
//! }
//! ```

tokio::task_local! {
    static WRITE_ORIGIN: WriteOrigin;
}

/// Origin of a skill write operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOrigin {
    /// Normal user-directed agent session.
    Foreground,
    /// Background self-improvement review fork.
    BackgroundReview,
}

impl std::fmt::Display for WriteOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Matches Hermes strings (background_review.py:310, agent_init.py:1014)
            // so that cross-runtime log/event consumers can pattern-match on
            // the same wire format.
            WriteOrigin::Foreground => write!(f, "assistant_tool"),
            WriteOrigin::BackgroundReview => write!(f, "background_review"),
        }
    }
}

impl WriteOrigin {
    /// Get the current write origin.
    ///
    /// Returns `Foreground` when called outside any `with_write_origin`
    /// scope (e.g., from a synchronous test or before the runtime has
    /// been entered).
    pub fn current() -> Self {
        WRITE_ORIGIN.try_with(|o| *o).unwrap_or(WriteOrigin::Foreground)
    }

    /// Check if currently running in background review context.
    pub fn is_background_review() -> bool {
        Self::current() == Self::BackgroundReview
    }
}

/// Run `fut` with `WRITE_ORIGIN` set to `origin` for the duration of
/// the future.
///
/// Nested scopes stack: an inner scope's value wins for code inside it,
/// and the outer value is restored when the inner scope ends. This is
/// equivalent to `WRITE_ORIGIN.scope(origin, fut).await` but exposed
/// as a free function so call sites don't need to import the
/// `tokio::task_local::TaskLocalFuture` machinery directly.
pub async fn with_write_origin<F, T>(origin: WriteOrigin, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    WRITE_ORIGIN.scope(origin, fut).await
}

/// Skill names that must never be deleted, archived, or moved to the
/// `Archived` state, regardless of who issued the call. Hermes parity
/// (`skill_usage.py:402-431`): `plan` is the only built-in whose
/// absence silently bricks the planning pipeline, so every mutating
/// path gates on this constant before the `is_agent_created` /
/// `WriteOrigin` checks. Adding to this list is the safe way to
/// whitelist a built-in — touching the gate functions in
/// `manage.rs` is not required.
pub const PROTECTED_BUILTIN_SKILLS: &[&str] = &["plan"];

/// Returns true if `name` is in [`PROTECTED_BUILTIN_SKILLS`]. Use this
/// helper everywhere a mutating path (archive_skill, set_state Archived,
/// storage::delete, …) would otherwise proceed. Centralising the check
/// here means adding a new protected skill is a single-line change.
pub fn is_protected_builtin(name: &str) -> bool {
    PROTECTED_BUILTIN_SKILLS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn default_origin_is_foreground() {
        assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
        assert!(!WriteOrigin::is_background_review());
    }

    #[tokio::test]
    async fn scope_sets_and_resets_origin() {
        assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
        let result = with_write_origin(WriteOrigin::BackgroundReview, async {
            assert_eq!(WriteOrigin::current(), WriteOrigin::BackgroundReview);
            assert!(WriteOrigin::is_background_review());
            42
        })
        .await;
        assert_eq!(result, 42);
        assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
    }

    #[tokio::test]
    async fn scope_survives_across_await() {
        with_write_origin(WriteOrigin::BackgroundReview, async {
            tokio::task::yield_now().await;
            assert_eq!(WriteOrigin::current(), WriteOrigin::BackgroundReview);
            tokio::task::yield_now().await;
            assert_eq!(WriteOrigin::current(), WriteOrigin::BackgroundReview);
        })
        .await;
    }

    #[tokio::test]
    async fn nested_scopes_restore_correctly() {
        with_write_origin(WriteOrigin::BackgroundReview, async {
            assert_eq!(WriteOrigin::current(), WriteOrigin::BackgroundReview);
            with_write_origin(WriteOrigin::Foreground, async {
                assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
            })
            .await;
            assert_eq!(WriteOrigin::current(), WriteOrigin::BackgroundReview);
        })
        .await;
        assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
    }

    // Note: `task_local!` does NOT propagate through `tokio::spawn` — the
    // spawned task is a fresh top-level task without the scope set up.
    // Our use case (the `call()` future, which contains the entire tool
    // body and is awaited in-place) does not span `tokio::spawn` calls,
    // so this limitation does not affect us. If a future feature needs
    // propagation to spawned tasks, switch to `LocalSet::spawn_local`.

    #[tokio::test]
    async fn return_value_passes_through_scope() {
        let v = with_write_origin(WriteOrigin::BackgroundReview, async {
            tokio::task::yield_now().await;
            "payload"
        })
        .await;
        assert_eq!(v, "payload");
    }

    #[test]
    fn display_foreground_eq_assistant_tool() {
        assert_eq!(WriteOrigin::Foreground.to_string(), "assistant_tool");
    }

    #[test]
    fn display_background_review_eq_background_review() {
        assert_eq!(
            WriteOrigin::BackgroundReview.to_string(),
            "background_review"
        );
    }
}