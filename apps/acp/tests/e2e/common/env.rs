//! `TestEnv` — isolated temp Loom home for one e2e case.
//!
//! Each `TestEnv::setup()` produces a fresh `TempDir`. The caller is
//! responsible for applying the home override for the duration of the test
//! (process-global override + explicit `--home` for spawned children).
//!
//! ```text
//! let env = TestEnv::setup();
//! with_loom_home(&env, || async {
//!     let h = AcpTestHarness::spawn(&env, &llm_url()).await;
//!     // ...
//! }).await;
//! ```
//!
//! The `loom acp` child spawned by `AcpTestHarness` receives `--home`
//! explicitly (the override is process state and does not propagate via
//! environment), so:
//!
//!   - sqlite checkpointer lands under `<tmp>/thread/<sid>/...`
//!   - PID file at   `<tmp>/acp/loom-acp.pid`
//!   - default log at `<tmp>/loom.log` (we override to `<tmp>/loom-acp.log`
//!     via `--log-file` for determinism — see `AcpTestHarness::spawn`).
//!
//! All e2e tests that mutate the home override MUST be marked
//! `#[serial_test::serial]` to avoid cross-test contamination of the
//! process-global override.

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
    // Release/compatibility runs can point the same wire-level harness at a
    // separately built Loom binary. Keep the normal Cargo-discovered path as
    // the default so local tests remain unchanged.
    if let Some(path) = std::env::var_os("LOOM_ACP_BINARY") {
        return PathBuf::from(path);
    }
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

/// Point the process-global home override at `env.loom_home()`, run `fut`,
/// restore prior value.
///
/// The override is process-global, so concurrent tests that touch it will
/// race. The mega + micro e2e tests all carry `#[serial_test::serial]` to
/// avoid that. Spawned `loom` children receive `--home` explicitly and do
/// not rely on this in-process override.
pub async fn with_loom_home<F, T>(env: &TestEnv, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let prev = config::home::override_path();
    config::home::set_override(Some(env.loom_home().to_path_buf()));
    let result = fut.await;
    config::home::set_override(prev);
    result
}

pub struct TestEnv {
    /// Root for the Loom home; loom-acp will write `acp/`, `logs/`, `thread/`
    /// underneath. Held to keep the directory alive for the test's duration.
    pub home: TempDir,
    /// Working folder passed as `cwd` to `session/new`.
    pub cwd: PathBuf,
}

impl TestEnv {
    /// Build a fresh `TestEnv` rooted at a unique `TempDir`. The caller must
    /// set the process home override (use `with_loom_home` to scope it).
    pub fn setup() -> Self {
        let home = tempfile::Builder::new()
            .prefix("loom-acp-e2e-")
            .tempdir()
            .expect("create tempdir");
        let cwd = home.path().join("cwd");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        Self { home, cwd }
    }

    /// Borrow the loom home root (e.g. for `--log-file <tmp>/loom-acp.log`).
    pub fn loom_home(&self) -> &Path {
        self.home.path()
    }
}
