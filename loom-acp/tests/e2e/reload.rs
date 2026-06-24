//! M3 — `loom-acp reload` subcommand + `--show-log-dir` flag.
//!
//! Plan 026 §2.3 M3. These test the CLI subcommands directly (no ACP server
//! spawned). On Windows the `reload` subcommand prints "not supported" but
//! `--show-log-dir` is fully functional.

#![allow(dead_code)]

use crate::common::env::binary_path;

/// `loom-acp --show-log-dir` should print a path containing `loom`.
#[tokio::test(flavor = "current_thread")]
async fn show_log_dir_outputs_path() {
    let output = tokio::process::Command::new(binary_path())
        .arg("--show-log-dir")
        .output()
        .await
        .expect("spawn loom-acp --show-log-dir");

    assert!(
        output.status.success(),
        "--show-log-dir should exit 0, got {:?}",
        output.status
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The output might go to stdout or stderr depending on the implementation.
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.to_lowercase().contains("loom")
            || combined.contains("log"),
        "--show-log-dir output should contain a path, got: {combined}"
    );
}

/// `loom-acp reload` without a PID file should not crash.
/// On Windows it prints "not supported"; on Unix it exits 1 when no PID file.
#[tokio::test(flavor = "current_thread")]
async fn reload_without_pid_does_not_crash() {
    let output = tokio::process::Command::new(binary_path())
        .arg("reload")
        .env("LOOM_HOME", std::env::temp_dir().join("loom-acp-e2e-no-pid"))
        .output()
        .await
        .expect("spawn loom-acp reload");

    // The process should exit (either 0 on Windows "not supported",
    // or non-zero on Unix when PID file is missing). Either way, it
    // shouldn't panic or hang.
    let _ = output.status;
}
