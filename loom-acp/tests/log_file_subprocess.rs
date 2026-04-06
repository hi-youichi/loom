//! Spawns the real `loom-acp` binary, completes ACP initialize + session/new, and asserts
//! that `--log-file '{working_folder}/logs/acp.log'` resolves and creates the log file.

#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
fn read_jsonrpc_response(
    reader: &mut impl BufRead,
    want_id: i64,
    deadline: Instant,
) -> serde_json::Value {
    let mut line = String::new();
    loop {
        if Instant::now() > deadline {
            panic!("timeout waiting for JSON-RPC response id={want_id}");
        }
        line.clear();
        let n = reader.read_line(&mut line).expect("read stdout");
        if n == 0 {
            panic!("unexpected EOF waiting for JSON-RPC response id={want_id}");
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(trimmed)
            .unwrap_or_else(|e| panic!("invalid JSON: {trimmed}: {e}"));
        if v.get("method").is_some() {
            // Agent -> client notification; ignore for this test.
            continue;
        }
        if v.get("id").and_then(|id| id.as_i64()) == Some(want_id) {
            return v;
        }
    }
}

#[test]
#[cfg(unix)]
fn log_file_resolves_working_folder_placeholder() {
    let temp = tempfile::tempdir().expect("tempdir");
    let loom_home = temp.path().join("loom_home");
    std::fs::create_dir_all(&loom_home).expect("create loom_home");

    let working_folder = temp.path().join("workspace");
    std::fs::create_dir_all(&working_folder).expect("create workspace");
    let working_folder = working_folder
        .canonicalize()
        .expect("canonicalize workspace");

    let expected_log = working_folder.join("logs").join("acp.log");
    assert!(
        !expected_log.exists(),
        "log file should not exist before session/new"
    );

    let cwd_str = working_folder.to_string_lossy().replace('\\', "\\\\");

    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {}
        }
    });
    let new_session = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "session/new",
        "params": {
            "cwd": cwd_str,
            "mcpServers": []
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_loom-acp"))
        .env("LOOM_HOME", &loom_home)
        .arg("--log-level")
        .arg("info")
        .arg("--log-file")
        .arg("{working_folder}/logs/acp.log")
        .arg("--log-rotate")
        .arg("none")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn loom-acp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    let deadline = Instant::now() + Duration::from_secs(120);

    writeln!(stdin, "{}", init.to_string()).expect("write init");
    stdin.flush().ok();
    let init_res = read_jsonrpc_response(&mut reader, 0, deadline);
    assert!(
        init_res.get("error").is_none(),
        "initialize failed: {}",
        init_res
    );

    writeln!(stdin, "{}", new_session.to_string()).expect("write session/new");
    stdin.flush().ok();
    let sess_res = read_jsonrpc_response(&mut reader, 1, deadline);
    assert!(
        sess_res.get("error").is_none(),
        "session/new failed: {}",
        sess_res
    );

    drop(stdin);

    // Drain remaining stdout so the child never blocks on write.
    let mut drain = String::new();
    while reader.read_line(&mut drain).expect("drain") > 0 {}

    let status = child.wait().expect("wait");
    assert!(status.success(), "loom-acp exit status: {status}");

    // Non-blocking tracing appender may lag slightly behind session/new return.
    let mut saw_log = false;
    for _ in 0..50 {
        if expected_log.is_file() {
            let meta = std::fs::metadata(&expected_log).expect("metadata");
            if meta.len() > 0 {
                saw_log = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        saw_log,
        "expected non-empty log at {}",
        expected_log.display()
    );

    let contents = std::fs::read_to_string(&expected_log).expect("read log");
    assert!(
        contents.contains("Listed all available models"),
        "expected ModelRegistry log line; got {} bytes:\n{}",
        contents.len(),
        &contents.chars().take(2000).collect::<String>()
    );
}
