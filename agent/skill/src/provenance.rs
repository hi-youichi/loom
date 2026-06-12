//! Skill write-origin provenance — task-local context for distinguishing
//! background review writes from foreground user-directed writes.
//!
//! The curator only consolidates/prunes skills it autonomously created via
//! background self-improvement review. Skills a user asks a foreground
//! agent to write belong to the user and must never be auto-curated.
//!
//! # Usage
//!
//! ```ignore
//! use skill::provenance::{WriteOrigin, WriteOriginGuard};
//!
//! // Wrap a section of code in background-review context:
//! let _guard = WriteOriginGuard::new(WriteOrigin::BackgroundReview);
//! // ... tool runs here — skill_create will see BackgroundReview origin
//!
//! // Inside a tool handler:
//! let origin = WriteOrigin::current();
//! if matches!(origin, WriteOrigin::BackgroundReview) {
//!     store.mark_agent_created(name);
//! }
//! ```

use std::cell::RefCell;

thread_local! {
    static WRITE_ORIGIN: RefCell<WriteOrigin> = const { RefCell::new(WriteOrigin::Foreground) };
}

/// Origin of a skill write operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOrigin {
    /// Normal user-directed agent session.
    Foreground,
    /// Background self-improvement review fork.
    BackgroundReview,
}

impl WriteOrigin {
    /// Get the current write origin.
    pub fn current() -> Self {
        WRITE_ORIGIN.with(|cell| *cell.borrow())
    }

    /// Check if currently running in background review context.
    pub fn is_background_review() -> bool {
        Self::current() == Self::BackgroundReview
    }
}

/// RAII guard that sets the write origin and resets on drop.
pub struct WriteOriginGuard {
    previous: WriteOrigin,
}

impl WriteOriginGuard {
    /// Set the write origin for the current scope. Resets when dropped.
    pub fn new(origin: WriteOrigin) -> Self {
        let previous = WRITE_ORIGIN.with(|cell| {
            let prev = *cell.borrow();
            *cell.borrow_mut() = origin;
            prev
        });
        Self { previous }
    }
}

impl Drop for WriteOriginGuard {
    fn drop(&mut self) {
        WRITE_ORIGIN.with(|cell| {
            *cell.borrow_mut() = self.previous;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_origin_is_foreground() {
        assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
        assert!(!WriteOrigin::is_background_review());
    }

    #[test]
    fn guard_sets_and_resets_origin() {
        assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
        {
            let _guard = WriteOriginGuard::new(WriteOrigin::BackgroundReview);
            assert_eq!(WriteOrigin::current(), WriteOrigin::BackgroundReview);
            assert!(WriteOrigin::is_background_review());
        }
        assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
    }

    #[test]
    fn nested_guards_restore_correctly() {
        let _g1 = WriteOriginGuard::new(WriteOrigin::BackgroundReview);
        assert_eq!(WriteOrigin::current(), WriteOrigin::BackgroundReview);
        {
            let _g2 = WriteOriginGuard::new(WriteOrigin::Foreground);
            assert_eq!(WriteOrigin::current(), WriteOrigin::Foreground);
        }
        assert_eq!(WriteOrigin::current(), WriteOrigin::BackgroundReview);
    }
}
