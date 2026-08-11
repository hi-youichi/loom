//! 测试共享工具：环境变量操作的进程级互斥锁。
//!
//! Rust 测试默认并行执行；多个测试文件同时 `std::env::set_var`/`remove_var`
//! 操作同一组环境变量会互相覆盖导致 flaky（如 `LOOM_MEMORY_NUDGE_INTERVAL`）。
//! 所有 env 相关测试共享此锁，串行化环境变量读写。
#![allow(clippy::missing_const_for_thread_local)] // 已用 const 块初始化，clippy 1.96 误报

use std::sync::{Mutex, OnceLock};

/// 进程级环境变量互斥锁（crate 内所有 env 测试共享）。
pub(crate) fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 在闭包内设置/恢复环境变量。
///
/// 与 `env_lock()` 互斥；同一线程嵌套调用不会死锁（线程局部重入计数），
/// panic 时通过 RAII 恢复环境变量并释放锁。
pub(crate) fn with_env(key: &str, value: Option<&str>, f: impl FnOnce()) {
    thread_local! {
        static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    // 仅线程上最外层调用持有进程锁（嵌套调用复用已持锁）。
    let is_outer = DEPTH.with(|d| {
        let depth = d.get();
        d.set(depth + 1);
        depth == 0
    });
    let _guard = is_outer.then(|| env_lock().lock().unwrap_or_else(|e| e.into_inner()));

    let prev = std::env::var(key).ok();
    match value {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }

    struct Restore<'a> {
        key: &'a str,
        prev: Option<String>,
    }
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
            thread_local! {
                static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
            }
            DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
    let _restore = Restore { key, prev };
    f();
}
