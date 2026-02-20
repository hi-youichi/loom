use std::process::Command;

fn run_loom(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(args)
        .output()
        .expect("failed to run loom binary")
}

#[test]
fn cli_help_succeeds() {
    let out = run_loom(&["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Loom"));
    assert!(stdout.contains("tool"));
}

#[test]
fn cli_tool_list_local_json_succeeds() {
    let out = run_loom(&["--local", "--json", "tool", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim_start().starts_with('['));
    assert!(stdout.contains("\"name\""));
}

#[test]
fn cli_tool_show_existing_local_json_succeeds() {
    let out = run_loom(&["--local", "--json", "tool", "show", "get_recent_messages"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"name\""));
    assert!(stdout.contains("get_recent_messages"));
}

#[test]
fn cli_tool_show_missing_local_fails() {
    let out = run_loom(&["--local", "tool", "show", "no_such_tool"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("tool not found"));
}

#[test]
fn cli_remote_no_auto_start_reports_connection_error() {
    let out = run_loom(&[
        "--remote",
        "ws://127.0.0.1:1",
        "--no-auto-start",
        "tool",
        "list",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        stderr.contains("refused")
            || stderr.contains("connection")
            || stderr.contains("connect")
    );
}
