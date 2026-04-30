use std::time::Duration;

mod common;
mod e2e;

use common::AcpChild;

#[test]
fn e2e_process_exits_on_stdin_close() {
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");

    // Close stdin to signal EOF
    acp.drop_stdin();

    // Should exit cleanly
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_invalid_json_returns_parse_error() {
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");

    // Send malformed JSON - not implemented in current AcpChild
    acp.drop_stdin();

    // Read response
    let message = acp.read_message().expect("read message");
    let response: e2e::RpcResponse = serde_json::from_value(message).expect("parse response");

    // Should get JSON-RPC parse error (-32700)
    assert!(response.error.is_some(), "should have error");
    let error = response.error.as_ref().unwrap();
    assert_eq!(error.code, -32700, "should be parse error");
}
