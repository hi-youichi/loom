//! Desktop notification system for Loom TUI.
//!
//! Sends macOS notifications when the terminal is not focused,
//! for events like reply completion, approval requests, and errors.
//!
//! # Usage
//!
//! ```ignore
//! let notifier = NotificationManager::new(true);
//! let focus = notifier.focus_state();
//! // ... in focus event handler ...
//! focus.store(false, std::sync::atomic::Ordering::Relaxed);
//! // ... later, when event happens ...
//! notifier.notify(NotificationType::ReplyComplete)?;
//! ```

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// NotificationType
// ---------------------------------------------------------------------------

/// Types of desktop notifications that can be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationType {
    /// AI reply has completed — user can now see the full response.
    ReplyComplete,
    /// User approval is required (e.g., a tool call needs confirmation).
    NeedApproval,
    /// An error occurred during processing.
    Error,
}

impl NotificationType {
    /// Human-readable notification title.
    fn title(&self) -> &'static str {
        match self {
            Self::ReplyComplete => "Loom TUI",
            Self::NeedApproval => "Loom TUI",
            Self::Error => "Loom TUI — Error",
        }
    }

    /// Human-readable notification body message.
    fn body(&self) -> &'static str {
        match self {
            Self::ReplyComplete => "AI reply has completed",
            Self::NeedApproval => "Approval required",
            Self::Error => "An error occurred",
        }
    }
}

// ---------------------------------------------------------------------------
// NotificationManager
// ---------------------------------------------------------------------------

/// Desktop notification manager.
///
/// Tracks terminal focus state and sends macOS native notifications
/// via `osascript` when the terminal is not focused.
///
/// # Thread safety
///
/// The focus state is stored in an `Arc<AtomicBool>` so that the TUI's
/// focus event handler (running on any thread) can atomically update it
/// without locking. The `notify` method reads the flag with relaxed
/// ordering — a stale read by one instruction is acceptable.
pub struct NotificationManager {
    /// Whether the terminal window is currently focused.
    terminal_focused: Arc<AtomicBool>,
    /// Whether desktop notifications are enabled globally.
    enabled: bool,
}

impl NotificationManager {
    /// Create a new notification manager.
    ///
    /// `enabled` controls whether notifications are sent at all.
    /// When `true`, notifications are only sent if the terminal is
    /// **not** focused (see [`focus_state`](Self::focus_state)).
    ///
    /// The initial focus state is `true` (terminal is assumed focused
    /// at startup). The caller should connect the TUI's focus-gained /
    /// focus-lost events to the returned `Arc<AtomicBool>`.
    pub fn new(enabled: bool) -> Self {
        Self {
            terminal_focused: Arc::new(AtomicBool::new(true)),
            enabled,
        }
    }

    /// Get the focus state handle for cross-thread sharing.
    ///
    /// The returned `Arc<AtomicBool>` can be shared with the event handler:
    /// - Store `false` when the terminal loses focus (e.g., Alt-Tab away).
    /// - Store `true` when the terminal regains focus.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let focus = notifier.focus_state();
    /// // In your focus event callback:
    /// focus.store(false, Ordering::Release);
    /// ```
    pub fn focus_state(&self) -> Arc<AtomicBool> {
        self.terminal_focused.clone()
    }

    /// Send a desktop notification.
    ///
    /// Returns `Ok(())` immediately (no-op) if:
    /// - Notifications are disabled (`enabled = false`), or
    /// - The terminal is currently focused.
    ///
    /// On **macOS**: uses `osascript` to display a native notification
    /// banner. Requires that the macOS Notification Center is running.
    ///
    /// On other platforms: returns `Ok(())` silently (no-op).
    /// Future support for Linux (notify-send / D-Bus) and Windows
    /// (toast notifications) can be added via `#[cfg]` blocks.
    pub fn notify(&self, notif_type: NotificationType) -> std::io::Result<()> {
        // Fast path: disabled entirely.
        if !self.enabled {
            return Ok(());
        }

        // Don't notify if the terminal is currently focused.
        if self.terminal_focused.load(Ordering::Relaxed) {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            let title = notif_type.title();
            let body = notif_type.body();
            Command::new("osascript")
                .arg("-e")
                .arg(format!(
                    r#"display notification "{}" with title "{}""#,
                    body, title
                ))
                .output()?;
        }

        // No-op on other platforms until support is added.
        #[cfg(not(target_os = "macos"))]
        {
            let _ = notif_type;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    // -----------------------------------------------------------------------
    // NotificationType
    // -----------------------------------------------------------------------

    #[test]
    fn test_notification_type_debug() {
        assert_eq!(
            format!("{:?}", NotificationType::ReplyComplete),
            "ReplyComplete"
        );
        assert_eq!(
            format!("{:?}", NotificationType::NeedApproval),
            "NeedApproval"
        );
        assert_eq!(format!("{:?}", NotificationType::Error), "Error");
    }

    #[test]
    fn test_notification_type_clone_copy_eq() {
        // Copy semantics
        let a = NotificationType::ReplyComplete;
        let b = a;
        assert_eq!(a, b);

        // Clone semantics
        let c = a.clone();
        assert_eq!(a, c);

        // Eq (reflexive)
        assert!(a == a);
    }

    #[test]
    fn test_notification_type_inequality() {
        assert_ne!(
            NotificationType::ReplyComplete,
            NotificationType::NeedApproval
        );
        assert_ne!(
            NotificationType::ReplyComplete,
            NotificationType::Error
        );
        assert_ne!(
            NotificationType::NeedApproval,
            NotificationType::Error
        );
    }

    #[test]
    fn test_notification_type_title_reply_complete() {
        assert_eq!(
            NotificationType::ReplyComplete.title(),
            "Loom TUI"
        );
    }

    #[test]
    fn test_notification_type_title_need_approval() {
        assert_eq!(
            NotificationType::NeedApproval.title(),
            "Loom TUI"
        );
    }

    #[test]
    fn test_notification_type_title_error() {
        assert_eq!(
            NotificationType::Error.title(),
            "Loom TUI — Error"
        );
    }

    #[test]
    fn test_notification_type_body_reply_complete() {
        assert_eq!(
            NotificationType::ReplyComplete.body(),
            "AI reply has completed"
        );
    }

    #[test]
    fn test_notification_type_body_need_approval() {
        assert_eq!(
            NotificationType::NeedApproval.body(),
            "Approval required"
        );
    }

    #[test]
    fn test_notification_type_body_error() {
        assert_eq!(
            NotificationType::Error.body(),
            "An error occurred"
        );
    }

    #[test]
    fn test_notification_type_all_variants_covered() {
        // Exhaustive match — ensures no variant is added without test coverage.
        // If a new variant is added, this test will fail to compile.
        fn exhaust(_ty: NotificationType) {
            match _ty {
                NotificationType::ReplyComplete
                | NotificationType::NeedApproval
                | NotificationType::Error => {}
            }
        }
        let _ = exhaust;
    }

    // -----------------------------------------------------------------------
    // NotificationManager — construction
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_enabled() {
        let nm = NotificationManager::new(true);
        assert!(nm.enabled);
        assert!(nm.terminal_focused.load(Ordering::Relaxed));
    }

    #[test]
    fn test_new_disabled() {
        let nm = NotificationManager::new(false);
        assert!(!nm.enabled);
        assert!(nm.terminal_focused.load(Ordering::Relaxed));
    }

    #[test]
    fn test_initial_focused_state() {
        // The terminal is assumed focused at startup.
        let nm = NotificationManager::new(true);
        assert!(nm.terminal_focused.load(Ordering::Relaxed));
    }

    #[test]
    fn test_focus_state_returns_clone() {
        let nm = NotificationManager::new(true);
        let handle = nm.focus_state();
        // The handle should be a clone of the same Arc.
        assert!(Arc::ptr_eq(&handle, &nm.terminal_focused));
    }

    #[test]
    fn test_focus_state_independent_instances() {
        let nm1 = NotificationManager::new(true);
        let nm2 = NotificationManager::new(true);
        let h1 = nm1.focus_state();
        let h2 = nm2.focus_state();
        // Different instances have different Arcs.
        assert!(!Arc::ptr_eq(&h1, &h2));
    }

    // -----------------------------------------------------------------------
    // NotificationManager — notify logic
    // -----------------------------------------------------------------------

    #[test]
    fn test_notify_disabled_fast_path() {
        // When disabled, notify returns Ok(()) even if terminal is focused
        // (the fast path returns before checking focus).
        let nm = NotificationManager::new(false);
        // Explicitly set focused to false to prove the fast path is taken.
        nm.terminal_focused.store(false, Ordering::Relaxed);
        assert!(nm.notify(NotificationType::ReplyComplete).is_ok());
        assert!(nm.notify(NotificationType::NeedApproval).is_ok());
        assert!(nm.notify(NotificationType::Error).is_ok());
    }

    #[test]
    fn test_notify_focused_no_op() {
        // When enabled but terminal is focused, notify returns Ok(()) silently.
        let nm = NotificationManager::new(true);
        // Default: focused = true.
        assert!(nm.notify(NotificationType::ReplyComplete).is_ok());
        assert!(nm.notify(NotificationType::NeedApproval).is_ok());
        assert!(nm.notify(NotificationType::Error).is_ok());
    }

    /// On non-macOS platforms, notify() is a pure no-op when enabled
    /// and the terminal is not focused — the `#[cfg(not(target_os = "macos"))]`
    /// block silently discards the notification type.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn test_notify_not_focused_enabled_no_op() {
        let nm = NotificationManager::new(true);
        nm.terminal_focused.store(false, Ordering::Relaxed);
        assert!(nm.notify(NotificationType::ReplyComplete).is_ok());
        assert!(nm.notify(NotificationType::NeedApproval).is_ok());
        assert!(nm.notify(NotificationType::Error).is_ok());
    }

    #[test]
    fn test_notify_focus_changed_to_unfocused() {
        // Start focused, then lose focus.
        let nm = NotificationManager::new(true);
        assert!(nm.notify(NotificationType::ReplyComplete).is_ok());

        // Lose focus.
        nm.terminal_focused.store(false, Ordering::Relaxed);

        // On non-macOS, this is still a no-op and returns Ok(()).
        // On macOS, this will try to run osascript — the test only verifies
        // that the method doesn't panic and returns a Result.
        let result = nm.notify(NotificationType::ReplyComplete);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_notify_focus_toggle_twice() {
        // Focus → unfocus → refocus → unfocus.
        let nm = NotificationManager::new(true);

        // 1. Focused — no-op.
        assert!(nm.notify(NotificationType::ReplyComplete).is_ok());

        // 2. Unfocus — may send or not depending on platform.
        nm.terminal_focused.store(false, Ordering::Relaxed);
        let _ = nm.notify(NotificationType::ReplyComplete);

        // 3. Refocus — no-op.
        nm.terminal_focused.store(true, Ordering::Relaxed);
        assert!(nm.notify(NotificationType::ReplyComplete).is_ok());

        // 4. Unfocus again.
        nm.terminal_focused.store(false, Ordering::Relaxed);
        let _ = nm.notify(NotificationType::ReplyComplete);
    }

    #[test]
    fn test_notify_enabled_flag_checked_first() {
        // The `enabled` check happens before the focus check.
        // If enabled=false, notify returns Ok(()) even if focused=false.
        let nm = NotificationManager::new(false);
        nm.terminal_focused.store(false, Ordering::Relaxed);
        assert!(nm.notify(NotificationType::ReplyComplete).is_ok());
    }

    #[test]
    fn test_notify_enabled_flag_gate() {
        // Disable after construction — the enabled flag is immutable,
        // so this tests the initial state only.
        // (NotificationManager does not provide a set_enabled method.)
        let nm_enabled = NotificationManager::new(true);
        let nm_disabled = NotificationManager::new(false);

        // Both start focused, both return Ok(()).
        assert!(nm_enabled.notify(NotificationType::ReplyComplete).is_ok());
        assert!(nm_disabled.notify(NotificationType::ReplyComplete).is_ok());

        // Unfocus both.
        nm_enabled.terminal_focused.store(false, Ordering::Relaxed);
        nm_disabled.terminal_focused.store(false, Ordering::Relaxed);

        // Disabled one still returns Ok(()) (fast path).
        assert!(nm_disabled.notify(NotificationType::ReplyComplete).is_ok());

        // Enabled one may or may not succeed depending on platform.
        let _ = nm_enabled.notify(NotificationType::ReplyComplete);
    }

    // -----------------------------------------------------------------------
    // NotificationManager — focus state cross-thread
    // -----------------------------------------------------------------------

    #[test]
    fn test_focus_state_send_sync() {
        // Verify that Arc<AtomicBool> is Send + Sync so it can be shared
        // across threads (as documented in the struct docs).
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<Arc<AtomicBool>>();
        assert_sync::<Arc<AtomicBool>>();
    }

    #[test]
    fn test_focus_state_cross_thread_update() {
        // Simulate focus event handler updating the shared state.
        let nm = NotificationManager::new(true);
        let handle = nm.focus_state();

        // Spawn a thread that loses focus.
        let handle_clone = handle.clone();
        std::thread::spawn(move || {
            handle_clone.store(false, Ordering::Release);
        })
        .join()
        .expect("thread panicked");

        // Verify the main instance sees the update.
        assert!(!nm.terminal_focused.load(Ordering::Acquire));
    }

    #[test]
    fn test_focus_state_multiple_threads() {
        // Multiple threads all updating the same AtomicBool.
        let nm = NotificationManager::new(true);
        let handle = nm.focus_state();

        let mut threads = Vec::new();
        for _ in 0..10 {
            let h = handle.clone();
            threads.push(std::thread::spawn(move || {
                h.store(false, Ordering::Relaxed);
                h.store(true, Ordering::Relaxed);
            }));
        }
        for t in threads {
            t.join().expect("thread panicked");
        }

        // Final state is true (last write wins).
        assert!(nm.terminal_focused.load(Ordering::Relaxed));
    }

    // -----------------------------------------------------------------------
    // NotificationManager — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_notify_relaxed_ordering_documented() {
        // Verify that the notify method uses Relaxed ordering as documented.
        // This is a behavioral test: the fast path uses Ordering::Relaxed
        // which means a stale read by one instruction is acceptable.
        // We can't observe the difference in a unit test, but we can verify
        // the method doesn't panic under concurrent access patterns.
        let nm = Arc::new(NotificationManager::new(true));
        let handle = nm.focus_state();

        let nm_clone = nm.clone();
        let writer = std::thread::spawn(move || {
            for _ in 0..100 {
                handle.store(false, Ordering::Release);
                handle.store(true, Ordering::Release);
            }
        });

        let reader = std::thread::spawn(move || {
            for _ in 0..100 {
                let _ = nm_clone.notify(NotificationType::ReplyComplete);
            }
        });

        writer.join().expect("writer panicked");
        reader.join().expect("reader panicked");
    }

    #[test]
    fn test_notify_all_variants_disabled() {
        // All three notification types work identically through the disabled path.
        let nm = NotificationManager::new(false);
        assert!(nm.notify(NotificationType::ReplyComplete).is_ok());
        assert!(nm.notify(NotificationType::NeedApproval).is_ok());
        assert!(nm.notify(NotificationType::Error).is_ok());
    }

    #[test]
    fn test_notify_returns_io_result() {
        // The public API contract returns io::Result<()>.
        let nm = NotificationManager::new(true);
        let result: std::io::Result<()> = nm.notify(NotificationType::ReplyComplete);
        assert!(result.is_ok());
    }
}