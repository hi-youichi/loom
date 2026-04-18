use std::time::Duration;

mod e2e;

#[test]
fn e2e_process_exits_on_stdin_close() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    // Close stdin to signal EOF
    drop(acp.stdin);

    // Should exit cleanly
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_invalid_json_returns_parse_error() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    // Send malformed JSON
    writeln!(acp.stdin, "invalid json").unwrap();

    // Read response
    let message = acp.read_message().expect("read message");
    let response: e2e::RpcResponse = serde_json::from_value(message).expect("parse response");

    // Should get JSON-RPC parse error (-32700)
    assert!(response.error.is_some(), "should have error");
    let error = response.error.as_ref().unwrap();
    assert_eq!(error.code, -32700, "should be parse error");
}
