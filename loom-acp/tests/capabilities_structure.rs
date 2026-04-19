mod e2e;

use std::time::Duration;

fn initialize(acp: &mut e2e::AcpChild) -> serde_json::Map<String, serde_json::Value> {
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            Duration::from_secs(10),
        )
        .expect("initialize response");

    assert!(
        response.error.is_none(),
        "initialize should succeed: {:?}",
        response.error
    );
    response
        .result
        .expect("should have result")
        .as_object()
        .expect("result should be object")
        .clone()
}

#[test]
fn e2e_agent_capabilities_completeness() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let capabilities = result
        .get("agentCapabilities")
        .and_then(|v| v.as_object())
        .expect("should have agentCapabilities");

    let expected_top_level = [
        "loadSession",
        "sessionCapabilities",
        "promptCapabilities",
        "mcpCapabilities",
    ];
    for key in &expected_top_level {
        assert!(
            capabilities.contains_key(*key),
            "agentCapabilities should have '{}'",
            key
        );
    }

    for key in capabilities.keys() {
        assert!(
            expected_top_level.contains(&key.as_str()),
            "unexpected top-level field in agentCapabilities: '{}'",
            key
        );
    }
}

#[test]
fn e2e_agent_capabilities_field_types() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let capabilities = result
        .get("agentCapabilities")
        .and_then(|v| v.as_object())
        .expect("should have agentCapabilities");

    assert!(
        capabilities
            .get("loadSession")
            .and_then(|v| v.as_bool())
            .is_some(),
        "loadSession should be boolean"
    );
    assert!(
        capabilities
            .get("sessionCapabilities")
            .and_then(|v| v.as_object())
            .is_some(),
        "sessionCapabilities should be object"
    );
    assert!(
        capabilities
            .get("promptCapabilities")
            .and_then(|v| v.as_object())
            .is_some(),
        "promptCapabilities should be object"
    );
    if let Some(mcp_caps) = capabilities.get("mcpCapabilities") {
        assert!(
            mcp_caps.as_object().is_some(),
            "mcpCapabilities should be object"
        );
    }
}

#[test]
fn e2e_load_session_value() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let load_session = result
        .get("agentCapabilities")
        .and_then(|v| v.get("loadSession"))
        .and_then(|v| v.as_bool())
        .expect("loadSession should be boolean");

    assert!(load_session, "loadSession should be true");
}

#[test]
fn e2e_session_capabilities_structure() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let session_caps = result
        .get("agentCapabilities")
        .and_then(|v| v.get("sessionCapabilities"))
        .and_then(|v| v.as_object())
        .expect("should have sessionCapabilities");

    let expected = ["list", "fork"];
    for key in &expected {
        assert!(
            session_caps.contains_key(*key),
            "sessionCapabilities should have '{}'",
            key
        );
    }
}

#[test]
fn e2e_session_capabilities_list_is_object() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let list = result
        .get("agentCapabilities")
        .and_then(|v| v.get("sessionCapabilities"))
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_object())
        .expect("sessionCapabilities.list should be object");

    assert!(
        list.is_empty(),
        "sessionCapabilities.list should be empty object"
    );
}

#[test]
fn e2e_session_capabilities_fork_is_object() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let fork = result
        .get("agentCapabilities")
        .and_then(|v| v.get("sessionCapabilities"))
        .and_then(|v| v.get("fork"))
        .and_then(|v| v.as_object())
        .expect("sessionCapabilities.fork should be object");

    assert!(
        fork.is_empty(),
        "sessionCapabilities.fork should be empty object"
    );
}

#[test]
fn e2e_session_capabilities_no_extra_fields() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let session_caps = result
        .get("agentCapabilities")
        .and_then(|v| v.get("sessionCapabilities"))
        .and_then(|v| v.as_object())
        .expect("should have sessionCapabilities");

    let expected = ["list", "fork"];
    for key in session_caps.keys() {
        assert!(
            expected.contains(&key.as_str()),
            "unexpected field in sessionCapabilities: '{}'",
            key
        );
    }
    assert_eq!(session_caps.len(), expected.len());
}

#[test]
fn e2e_auth_methods_exists() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    assert!(
        result.get("authMethods").is_some(),
        "initialize response should have authMethods"
    );
}

#[test]
fn e2e_auth_methods_is_array() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let auth_methods = result
        .get("authMethods")
        .and_then(|v| v.as_array())
        .expect("authMethods should be array");

    assert!(
        auth_methods.is_empty(),
        "authMethods should be empty array when no auth configured"
    );
}

#[test]
fn e2e_agent_info_name_is_loom() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let name = result
        .get("agentInfo")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .expect("agentInfo.name should be string");

    assert_eq!(name, "loom");
}

#[test]
fn e2e_agent_info_version_semver() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let version = result
        .get("agentInfo")
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .expect("agentInfo.version should be string");

    let parts: Vec<&str> = version.split('.').collect();
    assert!(
        parts.len() >= 2,
        "version should be semver format (x.y.z), got: '{}'",
        version
    );
    for part in &parts {
        assert!(
            part.parse::<u32>().is_ok(),
            "version segment '{}' should be numeric",
            part
        );
    }
}

#[test]
fn e2e_agent_info_title_when_present() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let agent_info = result
        .get("agentInfo")
        .and_then(|v| v.as_object())
        .expect("should have agentInfo");

    if let Some(title) = agent_info.get("title").and_then(|v| v.as_str()) {
        assert!(
            !title.is_empty(),
            "agentInfo.title should not be empty when present"
        );
    }
}

#[test]
fn e2e_agent_info_no_extra_fields() {
    let mut acp = e2e::AcpChild::spawn(None).expect("spawn loom-acp");
    let result = initialize(&mut acp);

    let agent_info = result
        .get("agentInfo")
        .and_then(|v| v.as_object())
        .expect("should have agentInfo");

    let expected = ["name", "version"];
    for key in agent_info.keys() {
        assert!(
            expected.contains(&key.as_str()) || key == "title" || key == "_meta",
            "unexpected field in agentInfo: '{}'",
            key
        );
    }
}
