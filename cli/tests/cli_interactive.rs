use std::io::Write;
use std::process::{Command, Stdio};

fn run_with_stdin(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn loom");

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("failed to write stdin");
    }

    child.wait_with_output().expect("failed to wait output")
}

#[test]
fn interactive_quit_immediately_exits_success() {
    let out = run_with_stdin(&["--local", "-i"], "quit\n");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Bye."));
}

#[test]
fn interactive_empty_line_then_quit_exits_success() {
    let out = run_with_stdin(&["--local", "-i"], "\nquit\n");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Bye."));
}

#[test]
fn interactive_initial_message_with_invalid_working_folder_exits_error() {
    let out = run_with_stdin(
        &[
            "--local",
            "-i",
            "-m",
            "hello",
            "--working-folder",
            "/definitely/not/exist/loom-cli-interactive-tests",
        ],
        "",
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(stderr.contains("error"));
}
