use std::time::Duration;

use crate::e2e;

#[test]
fn e2e_new_session_returns_session_id() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    // Handshake
    let session_id = acp.handshake(Duration::from_secs(10)).expect("handshake");

    // Verify session_id is non-empty
    assert!(!session_id.is_empty(), "session_id should not be empty");
}

#[test]
fn e2e_new_session_includes_modes() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    // Initialize
    acp.send_request_and_wait(
        "initialize",
        serde_json::json!({
            "protocolVersion": 1,
        }),
        Duration::from_secs(10),
    )
    .expect("initialize");

    // Create session
    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
            }),
            Duration::from_secs(10),
        )
        .expect("session/new response");

    let result = response.result.expect("should have result");

    // Verify modes
    let modes = result.get("modes").expect("should have modes");
    let available_modes = modes
        .get("availableModes")
        .and_then(|v| v.as_array())
        .expect("should have availableModes array");

    assert!(!available_modes.is_empty(), "should have at least one mode");

    // Verify mode structure
    for mode in available_modes {
        assert!(mode.get("id").is_some(), "mode should have id");
        assert!(mode.get("name").is_some(), "mode should have name");
    }
}

#[test]
fn e2e_new_session_includes_config_options() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");

    // Initialize
    acp.send_request_and_wait(
        "initialize",
        serde_json::json!({
            "protocolVersion": 1,
        }),
        Duration::from_secs(10),
    )
    .expect("initialize");

    // Create session
    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
            }),
            Duration::from_secs(10),
        )
        .expect("session/new response");

    let result = response.result.expect("should have result");

    // Verify configOptions
    let config_options = result
        .get("configOptions")
        .and_then(|v| v.as_array())
        .expect("should have configOptions array");

    assert!(
        !config_options.is_empty(),
        "should have at least one config option"
    );

    // Verify model option exists
    let has_model_option = config_options.iter().any(|opt| {
        opt.get("id")
            .and_then(|v| v.as_str())
            .map(|id| id == "model")
            .unwrap_or(false)
    });

    assert!(has_model_option, "should have model config option");
}
