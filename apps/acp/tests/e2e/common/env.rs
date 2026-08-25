//! `TestEnv` — isolated temp anureo home for one e2e case.
//!
//! Each `TestEnv::setup()` produces a fresh `TempDir`. The caller is
//! responsible for applying the home override for the duration of the test
//! (process-global override + explicit `--home` for spawned children).
//!
//! ```text
//! let env = TestEnv::setup();
//! with_anureo_home(&env, || async {
//!     let h = AcpTestHarness::spawn(&env, &llm_url()).await;
//!     // ...
//! }).await;
//! ```
//!
//! The `anureo acp` child spawned by `AcpTestHarness` receives `--home`
//! explicitly (the override is process state and does not propagate via
//! environment), so:
//!
//!   - sqlite checkpointer lands under `<tmp>/thread/<sid>/...`
//!   - PID file at   `<tmp>/acp/anureo-acp.pid`
//!   - default log at `<tmp>/anureo.log` (we override to `<tmp>/anureo-acp.log`
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

/// Resolved path to the `anureo` binary (built by the `cli` crate).
///
/// Since the `acp` crate no longer has its own binary, we derive the path
/// from `current_exe()` — test binaries live in `target/<profile>/deps/`,
/// so going up two parents gives `target/<profile>/` where `anureo` resides.
pub fn binary_path() -> PathBuf {
    // Release/compatibility runs can point the same wire-level harness at a
    // separately built anureo binary. Keep the normal Cargo-discovered path as
    // the default so local tests remain unchanged.
    if let Some(path) = std::env::var_os("ANUREO_ACP_BINARY") {
        return PathBuf::from(path);
    }
    if let Some(p) = option_env!("CARGO_BIN_EXE_anureo") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().expect("get current_exe");
    let bin_name = if cfg!(windows) { "anureo.exe" } else { "anureo" };
    let target_dir = exe
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| exe.parent().expect("test exe parent"));
    target_dir.join(bin_name)
}

/// Point the process-global home override at `env.anureo_home()`, run `fut`,
/// restore prior value.
///
/// The override is process-global, so concurrent tests that touch it will
/// race. The mega + micro e2e tests all carry `#[serial_test::serial]` to
/// avoid that. Spawned `anureo` children receive `--home` explicitly and do
/// not rely on this in-process override.
pub async fn with_anureo_home<F, T>(env: &TestEnv, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let prev = config::home::override_path();
    config::home::set_override(Some(env.anureo_home().to_path_buf()));
    let result = fut.await;
    config::home::set_override(prev);
    result
}

pub struct TestEnv {
    /// Root for the anureo home; anureo-acp will write `acp/`, `logs/`, `thread/`
    /// underneath. Held to keep the directory alive for the test's duration.
    pub home: TempDir,
    /// Working folder passed as `cwd` to `session/new`.
    pub cwd: PathBuf,
}

impl TestEnv {
    /// Build a fresh `TestEnv` rooted at a unique `TempDir`. The caller must
    /// set the process home override (use `with_anureo_home` to scope it).
    pub fn setup() -> Self {
        let home = tempfile::Builder::new()
            .prefix("anureo-acp-e2e-")
            .tempdir()
            .expect("create tempdir");
        let cwd = home.path().join("cwd");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        Self { home, cwd }
    }

    /// Borrow the anureo home root (e.g. for `--log-file <tmp>/anureo-acp.log`).
    pub fn anureo_home(&self) -> &Path {
        self.home.path()
    }
}
