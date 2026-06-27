//! `TestEnv` — isolated temp `LOOM_HOME` for one e2e case.
//!
//! Each `TestEnv::setup()` produces a fresh `TempDir`. The caller is
//! responsible for setting `LOOM_HOME` for the duration of the test (we
//! can't atomically scope env vars across async boundaries reliably, so
//! we use the simpler "set, run, restore" pattern in the test fn itself).
//!
//! ```text
//! let env = TestEnv::setup();
//! with_loom_home(&env, || async {
//!     let h = AcpTestHarness::spawn(&env, &llm_url()).await;
//!     // ...
//! }).await;
//! ```
//!
//! The `loom acp` child spawned by `AcpTestHarness` inherits the test
//! process's environment, so:
//!
//!   - sqlite checkpointer lands under `<tmp>/thread/<sid>/...`
//!   - PID file at   `<tmp>/acp/loom-acp.pid`
//!   - default log at `<tmp>/logs/acp/loom-acp.log` (we override to `<tmp>/loom-acp.log`
//!     via `--log-file` for determinism — see `AcpTestHarness::spawn`).
//!
//! All e2e tests that mutate `LOOM_HOME` MUST be marked `#[serial_test::serial]`
//! to avoid cross-test contamination of the process-global env.

// Phase 1 only exercises a small subset; allow dead code for items that
// Phase 2 / Phase 3 will use.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// Resolved path to the `loom` binary (built by the `cli` crate).
///
/// Since the `acp` crate no longer has its own binary, we derive the path
/// from `current_exe()` — test binaries live in `target/<profile>/deps/`,
/// so going up two parents gives `target/<profile>/` where `loom` resides.
pub fn binary_path() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_loom") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().expect("get current_exe");
    let bin_name = if cfg!(windows) { "loom.exe" } else { "loom" };
    let target_dir = exe
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| exe.parent().expect("test exe parent"));
    target_dir.join(bin_name)
}

/// Set `LOOM_HOME` to `env.loom_home()`, run `fut`, restore prior value.
///
/// Env-var mutation is process-global, so concurrent tests that touch
/// `LOOM_HOME` will race. The mega + micro e2e tests all carry
/// `#[serial_test::serial]` to avoid that.
pub async fn with_loom_home<F, T>(env: &TestEnv, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let prev = std::env::var("LOOM_HOME").ok();
    std::env::set_var("LOOM_HOME", env.loom_home());
    let result = fut.await;
    match prev {
        Some(v) => std::env::set_var("LOOM_HOME", v),
        None => std::env::remove_var("LOOM_HOME"),
    }
    result
}

pub struct TestEnv {
    /// Root for `LOOM_HOME`; loom-acp will write `acp/`, `logs/`, `thread/`
    /// underneath. Held to keep the directory alive for the test's duration.
    pub home: TempDir,
    /// Working folder passed as `cwd` to `session/new`.
    pub cwd: PathBuf,
}

impl TestEnv {
    /// Build a fresh `TestEnv` rooted at a unique `TempDir`. The caller must
    /// set `LOOM_HOME` (use `with_loom_home` to scope the env mutation).
    pub fn setup() -> Self {
        let home = tempfile::Builder::new()
            .prefix("loom-acp-e2e-")
            .tempdir()
            .expect("create tempdir");
        let cwd = home.path().join("cwd");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        Self { home, cwd }
    }

    /// Borrow the LOOM_HOME root (e.g. for `--log-file <tmp>/loom-acp.log`).
    pub fn loom_home(&self) -> &Path {
        self.home.path()
    }
}