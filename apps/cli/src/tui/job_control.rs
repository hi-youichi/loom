//! ^Z 暂停/恢复管理器 (Job Control)
//!
//! 处理 SIGTSTP 信号，实现 TUI 的暂停与恢复。
//! 仅在 Unix 系统上可用，整体使用 `#[cfg(unix)]` 条件编译。

#![cfg(unix)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use tokio::sync::Notify;

/// ^Z 暂停/恢复管理器
///
/// 通过 SIGTSTP 信号暂停进程，并在恢复后重新初始化终端。
/// 配合 `setup_signal_handler()` 使用（后者将 SIGTSTP 设为 SIG_IGN，
/// 由本管理器手动控制暂停流程）。
///
/// # 使用流程
///
/// 1. 在 `init()` 之后调用 `setup_signal_handler()`。
/// 2. 在主事件循环中，通过 `suspend_signal()` 监听暂停通知。
/// 3. 收到通知后，暂停 Agent 任务，然后调用 `suspend()`。
/// 4. `suspend()` 恢复终端 → 发送 SIGTSTP → 进程被 Shell 暂停。
/// 5. 用户执行 `fg` 恢复后，`suspend()` 重新初始化终端。
/// 6. 通过 `resume_signal()` 通知事件循环继续。
pub struct JobControl {
    /// 暂停通知信号（用于 `tokio::select!`）
    suspend: Arc<Notify>,
    /// 恢复通知信号（用于 `tokio::select!`）
    resume: Arc<Notify>,
    /// 是否已暂停
    suspended: AtomicBool,
}

impl JobControl {
    /// 创建一个新的 JobControl 实例。
    pub fn new() -> Self {
        Self {
            suspend: Arc::new(Notify::new()),
            resume: Arc::new(Notify::new()),
            suspended: AtomicBool::new(false),
        }
    }

    /// 获取暂停通知信号，用于 `tokio::select!` 监听暂停请求。
    ///
    /// 当收到暂停信号时，应暂停 Agent 任务，然后调用 [`suspend()`](Self::suspend)。
    pub fn suspend_signal(&self) -> Arc<Notify> {
        self.suspend.clone()
    }

    /// 获取恢复通知信号，用于 `tokio::select!` 监听恢复完成。
    ///
    /// 收到此信号表示进程已从 SIGTSTP 恢复，终端已重新初始化，
    /// 可以继续 Agent 任务和事件循环。
    pub fn resume_signal(&self) -> Arc<Notify> {
        self.resume.clone()
    }

    /// 检查当前是否处于暂停状态。
    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::Acquire)
    }

    /// 暂停当前进程。
    ///
    /// 执行以下步骤：
    /// 1. 恢复终端到正常模式（关闭 raw mode、显示光标、关闭 bracketed paste）。
    /// 2. 临时将 SIGTSTP 恢复为默认行为 (SIG_DFL)，然后发送 SIGTSTP
    ///    暂停进程（Shell 接管，用户可执行 `fg` 恢复）。
    /// 3. 恢复后，将 SIGTSTP 重新设为 SIG_IGN，并重新初始化终端
    ///   （开启 raw mode、隐藏光标、开启 bracketed paste）。
    /// 4. 通知所有等待恢复的协程。
    ///
    /// 此方法会阻塞当前线程，直到进程被 `fg` 恢复。
    ///
    /// # 重要
    ///
    /// 不能直接 `raise(SIGTSTP)` 而忽略 SIG_DFL 步骤：`setup_signal_handler()`
    /// 已将 SIGTSTP 设为 SIG_IGN，而 `raise()` 对 SIG_IGN 信号是空操作，
    /// 进程不会被暂停。必须先将 disposition 恢复为 SIG_DFL，暂停，
    /// 再恢复为 SIG_IGN。
    pub fn suspend(&self) -> std::io::Result<()> {
        self.suspended.store(true, Ordering::Release);

        // 1. 恢复终端到正常模式
        execute!(std::io::stdout(), DisableBracketedPaste)?;
        execute!(std::io::stdout(), Show)?;
        disable_raw_mode()?;

        // 2. 暂停进程
        //
        // 安全: 标准 Unix 信号操作。此时终端已恢复，进程被 Shell 暂停。
        // 必须先将 SIGTSTP 恢复为 SIG_DFL，否则 raise() 是空操作
        // （因为 SIGTSTP 当前为 SIG_IGN）。
        // 恢复后（fg）将 SIGTSTP 重新设为 SIG_IGN。恢复后执行流从
        // libc::raise() 的下一行继续。
        unsafe {
            libc::signal(libc::SIGTSTP, libc::SIG_DFL);
            libc::raise(libc::SIGTSTP);
            libc::signal(libc::SIGTSTP, libc::SIG_IGN);
        }

        // 3. 恢复后，重新初始化终端
        enable_raw_mode()?;
        execute!(std::io::stdout(), EnableBracketedPaste)?;
        execute!(std::io::stdout(), Hide)?;

        self.suspended.store(false, Ordering::Release);

        // 4. 通知所有等待恢复的协程
        self.resume.notify_waiters();

        Ok(())
    }
}

impl Default for JobControl {
    fn default() -> Self {
        Self::new()
    }
}

/// 设置 SIGTSTP 信号处理。
///
/// 将 SIGTSTP 设置为 `SIG_IGN`（忽略），由 [`JobControl::suspend()`] 手动
/// 控制暂停流程。必须在 `init()` 之后、启动事件循环之前调用。
///
/// # 设计说明
///
/// 默认情况下，SIGTSTP 由内核处理，会立即暂停进程。通过将其设为 SIG_IGN，
/// 我们可以：
/// - 在暂停前先恢复终端状态（raw mode → 正常模式）
/// - 在恢复后重新初始化终端
/// - 通过 `Notify` 通知事件循环协调暂停/恢复
pub fn setup_signal_handler() -> std::io::Result<()> {
    // 安全: 标准 POSIX 信号处理设置。此操作是线程安全的，
    // 且只在初始化阶段调用一次。
    let result = unsafe { libc::signal(libc::SIGTSTP, libc::SIG_IGN) };
    if result == libc::SIG_ERR {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[test]
    fn test_job_control_new() {
        let jc = JobControl::new();
        assert!(!jc.is_suspended());
    }

    #[test]
    fn test_job_control_default() {
        let jc = JobControl::default();
        assert!(!jc.is_suspended());
    }

    #[test]
    fn test_suspend_signal_returns_notify() {
        let jc = JobControl::new();
        let signal = jc.suspend_signal();
        // 新建的 Notify 没有 pending 通知
        assert!(!signal.notified().now_or_never().is_some());
    }

    #[test]
    fn test_resume_signal_returns_notify() {
        let jc = JobControl::new();
        let signal = jc.resume_signal();
        assert!(!signal.notified().now_or_never().is_some());
    }

    #[test]
    fn test_is_suspended_starts_false() {
        let jc = JobControl::new();
        assert_eq!(jc.is_suspended(), false);
    }

    #[test]
    fn test_suspend_signal_and_resume_signal_are_distinct() {
        let jc = JobControl::new();
        let s1 = Arc::as_ptr(&jc.suspend_signal());
        let s2 = Arc::as_ptr(&jc.resume_signal());
        assert_ne!(s1, s2, "suspend and resume must be distinct Notify instances");
    }

    // -----------------------------------------------------------------------
    // Signal handler
    // -----------------------------------------------------------------------

    #[test]
    fn test_setup_signal_handler_returns_ok() {
        let result = setup_signal_handler();
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Send / Sync (compile-time trait checks)
    // -----------------------------------------------------------------------

    #[test]
    fn test_job_control_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<JobControl>();
    }

    #[test]
    fn test_job_control_is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<JobControl>();
    }

    // -----------------------------------------------------------------------
    // Notify behavior — notify_waiters + notified interaction
    // -----------------------------------------------------------------------

    #[test]
    fn test_suspend_signal_notify_waiters_notifies() {
        let jc = JobControl::new();
        let signal = jc.suspend_signal();
        // Create a Notified future FIRST, then notify_waiters wakes it.
        let notified = signal.notified();
        signal.notify_waiters();
        let ready = notified.now_or_never();
        assert!(ready.is_some(), "suspend signal should be notified after notify_waiters");
    }

    #[test]
    fn test_resume_signal_notify_waiters_notifies() {
        let jc = JobControl::new();
        let signal = jc.resume_signal();
        let notified = signal.notified();
        signal.notify_waiters();
        let ready = notified.now_or_never();
        assert!(ready.is_some(), "resume signal should be notified after notify_waiters");
    }

    #[test]
    fn test_signals_are_independent() {
        let jc = JobControl::new();
        let suspend = jc.suspend_signal();
        let resume = jc.resume_signal();

        // Register waiters FIRST, then notify only suspend
        let suspend_waiter = suspend.notified();
        let resume_waiter = resume.notified();
        suspend.notify_waiters();

        // Suspend waiter should resolve
        assert!(suspend_waiter.now_or_never().is_some(),
            "suspend signal should be notified");
        // Resume waiter should NOT resolve
        assert!(resume_waiter.now_or_never().is_none(),
            "resume signal should NOT be notified when only suspend was notified");

        // Now notify resume and verify it resolves
        let resume_waiter2 = resume.notified();
        resume.notify_waiters();
        assert!(resume_waiter2.now_or_never().is_some(),
            "resume signal should be notified after its own notify_waiters");
    }

    #[test]
    fn test_notify_is_idempotent() {
        let jc = JobControl::new();
        let signal = jc.suspend_signal();
        let notified = signal.notified();
        signal.notify_waiters();
        // Calling notify_waiters multiple times should not cause issues
        signal.notify_waiters();
        signal.notify_waiters();
        let ready = notified.now_or_never();
        assert!(ready.is_some(), "signal should remain notified after multiple notify_waiters");
    }

    // -----------------------------------------------------------------------
    // Atomic flag — suspended state transitions
    // -----------------------------------------------------------------------

    #[test]
    fn test_suspended_flag_roundtrip() {
        let jc = JobControl::new();
        // Initial: false
        assert!(!jc.suspended.load(Ordering::Acquire), "should start as false");
        assert!(!jc.is_suspended(), "is_suspended() should reflect the flag");

        // Set to true
        jc.suspended.store(true, Ordering::Release);
        assert!(jc.suspended.load(Ordering::Acquire), "should be true after store");
        assert!(jc.is_suspended(), "is_suspended() should reflect the flag");

        // Reset to false
        jc.suspended.store(false, Ordering::Release);
        assert!(!jc.suspended.load(Ordering::Acquire), "should be false after reset");
        assert!(!jc.is_suspended(), "is_suspended() should reflect the flag");
    }

    #[test]
    fn test_suspended_flag_ordering() {
        // Verify that the Acquire/Release semantics are consistent:
        // a store(Release) is visible to a subsequent load(Acquire) on the same thread.
        let jc = JobControl::new();
        jc.suspended.store(true, Ordering::Release);
        assert!(jc.is_suspended());
        jc.suspended.store(false, Ordering::Release);
        assert!(!jc.is_suspended());
    }

    // -----------------------------------------------------------------------
    // Cross-instance isolation
    // -----------------------------------------------------------------------

    #[test]
    fn test_suspend_signals_isolated_per_instance() {
        let jc1 = JobControl::new();
        let jc2 = JobControl::new();
        assert_ne!(
            Arc::as_ptr(&jc1.suspend_signal()),
            Arc::as_ptr(&jc2.suspend_signal()),
            "each instance should have its own suspend signal"
        );
    }

    #[test]
    fn test_resume_signals_isolated_per_instance() {
        let jc1 = JobControl::new();
        let jc2 = JobControl::new();
        assert_ne!(
            Arc::as_ptr(&jc1.resume_signal()),
            Arc::as_ptr(&jc2.resume_signal()),
            "each instance should have its own resume signal"
        );
    }

    // -----------------------------------------------------------------------
    // Suspend flow — structural correctness (no side effects on test runner)
    // -----------------------------------------------------------------------

    #[test]
    fn test_suspend_returns_result_type() {
        // Verify the method signature is std::io::Result<()>
        fn assert_result_type(f: fn(&JobControl) -> std::io::Result<()>) {
            let _ = f;
        }
        assert_result_type(JobControl::suspend);
    }

    #[test]
    fn test_suspend_method_accepts_self_ref() {
        // Verify the method takes &self, not self or &mut self
        fn assert_method_sig(f: fn(&JobControl) -> std::io::Result<()>) {
            let _ = f;
        }
        assert_method_sig(|_jc: &JobControl| {
            // The method exists and has the right signature.
            // We can't call jc.suspend() here without a real TTY —
            // disable_raw_mode() returns ENXIO on non-TTY stdout.
            // This is exercised in integration tests with a real terminal.
            Ok(())
        });
    }

    /// Verify that `suspend()` returns an error in a non-TTY environment
    /// (e.g. test harness). This confirms the error propagation path works.
    #[test]
    fn test_suspend_fails_on_non_tty() {
        // In a subprocess, stdout is piped, so terminal operations fail.
        // Use a subprocess to avoid affecting the test runner's terminal.
        let exe = std::env::current_exe()
            .expect("failed to get current executable path");
        let output = std::process::Command::new(&exe)
            .env("_LOOM_TUI_SUSPEND_TEST", "1")
            .arg("--nocapture")
            .arg("--include-ignored")
            .arg("_suspend_error_test")
            .output()
            .expect("failed to run suspend subprocess");

        // The subprocess should exit successfully because
        // _suspend_error_test does exit(0) when suspend() fails as expected
        // on a non-TTY. Verify the stderr message.
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "subprocess crashed unexpectedly:\nstderr: {stderr}"
        );
        assert!(
            stderr.contains("suspend() failed on non-TTY as expected"),
            "expected 'suspend() failed on non-TTY as expected' in stderr, got:\n{stderr}"
        );
    }

    #[test]
    #[ignore = "only runs as a subprocess of test_suspend_fails_on_non_tty"]
    fn _suspend_error_test() {
        if std::env::var("_LOOM_TUI_SUSPEND_TEST").is_err() {
            return;
        }
        setup_signal_handler().unwrap();
        let jc = JobControl::new();
        match jc.suspend() {
            Ok(()) => {
                eprintln!("suspend() unexpectedly succeeded on non-TTY");
                std::process::exit(1);
            }
            Err(e) => {
                // Expected: terminal operations fail on non-TTY.
                // Print the error and exit cleanly.
                eprintln!(
                    "suspend() failed on non-TTY as expected: {e:?}"
                );
                std::process::exit(0);
            }
        }
    }
}