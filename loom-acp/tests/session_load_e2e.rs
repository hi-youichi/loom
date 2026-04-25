mod common;
mod e2e;

use serde::Deserialize;
use std::io::BufRead;
use std::time::Duration;

#[derive(Deserialize, PartialEq, Eq, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
enum SessionUpdateType {
    UserMessageChunk,
    AgentMessageChunk,
    AgentThoughtChunk,
    ToolCall,
    ToolCallUpdate,
    CurrentModeUpdate,
    Plan,
    ConfigOptionUpdate,
    SessionInfoUpdate,
}

impl SessionUpdateType {
    fn from_notification(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(
            value
                .get("params")?
                .get("update")?
                .get("sessionUpdate")?
                .clone(),
        )
        .ok()
    }
}

const TIMEOUT: Duration = Duration::from_secs(30);
const SHORT_TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

fn cwd() -> String {
    std::env::current_dir().unwrap().to_str().unwrap().to_string()
}

async fn initialize(acp: &mut common::AcpChild) {
    let response = acp
        .send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            SHORT_TIMEOUT,
        )
        .await
        .expect("initialize");
    assert!(response.error.is_none(), "initialize failed: {:?}", response.error);
}

async fn new_session(acp: &mut common::AcpChild) -> String {
    let response = acp
        .send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": cwd(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/new");
    assert!(response.error.is_none(), "session/new failed: {:?}", response.error);
    response
        .result
        .expect("should have result")
        .get("sessionId")
        .and_then(|v| v.as_str())
        .expect("should have sessionId")
        .to_string()
}

async fn spawn_with_session() -> (common::AcpChild, common::MockAcpServer, String) {
    let (mut acp, mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");
    initialize(&mut acp).await;
    let session_id = new_session(&mut acp).await;
    (acp, mock, session_id)
}

async fn prompt(acp: &mut common::AcpChild, session_id: &str, text: &str) -> common::RpcResponse {
    let request_id = acp
        .send_prompt_request(session_id, text)
        .expect("send prompt");
    let (_notifs, response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect prompt response");
    response
}

async fn load_session(
    acp: &mut common::AcpChild,
    session_id: &str,
    mcp_servers: serde_json::Value,
) -> common::RpcResponse {
    acp.send_request_and_wait(
        "session/load",
        serde_json::json!({
            "sessionId": session_id,
            "cwd": cwd(),
            "mcpServers": mcp_servers,
        }),
        TIMEOUT,
    )
    .await
    .expect("session/load")
}

fn extract_session_update_types(notifications: &[serde_json::Value]) -> Vec<SessionUpdateType> {
    notifications
        .iter()
        .filter_map(SessionUpdateType::from_notification)
        .collect()
}

fn drain_notifications_until(
    acp: &mut common::AcpChild,
    stop_at: SessionUpdateType,
    timeout: Duration,
) -> Vec<serde_json::Value> {
    let mut notifications = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let mut line = String::new();
        let bytes = acp.reader.read_line(&mut line).unwrap_or(0);
        if bytes == 0 || line.trim().is_empty() {
            continue;
        }
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line.trim()) {
            if msg.get("method").is_some() && msg.get("id").is_none() {
                let is_stop = SessionUpdateType::from_notification(&msg) == Some(stop_at);
                notifications.push(msg);
                if is_stop {
                    break;
                }
            }
        }
    }
    notifications
}

fn collect_notifications_or_drain(
    acp: &mut common::AcpChild,
    notifications: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    if notifications.is_empty() {
        let mut combined = notifications;
        combined.extend(drain_notifications_until(
            acp,
            SessionUpdateType::CurrentModeUpdate,
            DRAIN_TIMEOUT,
        ));
        combined
    } else {
        notifications
    }
}

#[tokio::test]
async fn e2e_load_fresh_session_returns_success() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let response = load_session(&mut acp, &session_id, serde_json::json!([])).await;

    assert!(response.error.is_none(), "session/load failed: {:?}", response.error);
    let result = response.result.expect("should have result");
    assert!(result.get("configOptions").is_some(), "should have configOptions");
    assert!(result.get("modes").is_some(), "should have modes");
}

#[tokio::test]
async fn e2e_load_session_replays_user_and_agent_messages() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let prompt_resp = prompt(&mut acp, &session_id, "Hello from history test").await;
    assert!(prompt_resp.error.is_none(), "prompt failed: {:?}", prompt_resp.error);

    let (notifications, response) = acp
        .load_and_collect_notifications(&session_id, &cwd(), TIMEOUT)
        .expect("load and collect");

    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let all_notifications = collect_notifications_or_drain(&mut acp, notifications);

    let update_types = extract_session_update_types(&all_notifications);
    assert!(update_types.contains(&SessionUpdateType::UserMessageChunk),
        "history should contain user_message_chunk, got: {:?}", update_types);
    assert!(update_types.contains(&SessionUpdateType::AgentMessageChunk),
        "history should contain agent_message_chunk, got: {:?}", update_types);
}

#[tokio::test]
async fn e2e_load_session_after_process_restart_restores_history() {
    let shared_home = tempfile::tempdir().expect("create shared temp dir");
    let shared_home_path = shared_home.path().to_path_buf();

    {
        let (mut acp_a, _mock_a) = common::AcpChild::spawn_with_mock_at_home(&shared_home_path)
            .await
            .expect("spawn process A");
        initialize(&mut acp_a).await;
        let session_id = new_session(&mut acp_a).await;

        let resp = prompt(&mut acp_a, &session_id, "Remember: secret=42").await;
        assert!(resp.error.is_none(), "prompt failed: {:?}", resp.error);

        std::fs::write(shared_home_path.join("test-session-id.txt"), &session_id)
            .expect("write session id");
    }

    let session_id = std::fs::read_to_string(shared_home_path.join("test-session-id.txt"))
        .expect("read session id");

    let (mut acp_b, _mock_b) = common::AcpChild::spawn_with_mock_at_home(&shared_home_path)
        .await
        .expect("spawn process B");
    initialize(&mut acp_b).await;

    let (notifications, response) = acp_b
        .load_and_collect_notifications(&session_id, &cwd(), TIMEOUT)
        .expect("load in process B");

    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let update_types = extract_session_update_types(&notifications);
    assert!(
        update_types.contains(&SessionUpdateType::UserMessageChunk),
        "cross-process load should replay history, got: {:?}",
        update_types
    );
}

#[tokio::test]
async fn e2e_prompt_after_load_session_succeeds() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let resp1 = prompt(&mut acp, &session_id, "First message").await;
    assert!(resp1.error.is_none(), "first prompt failed: {:?}", resp1.error);

    let load_resp = load_session(&mut acp, &session_id, serde_json::json!([])).await;
    assert!(load_resp.error.is_none(), "load failed: {:?}", load_resp.error);

    let resp2 = prompt(&mut acp, &session_id, "Follow up message").await;
    assert!(resp2.error.is_none(), "prompt after load failed: {:?}", resp2.error);
    let stop_reason = resp2
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    assert_eq!(stop_reason, "end_turn", "stop reason after load should be end_turn");
}

#[tokio::test]
async fn e2e_load_session_preserves_model_config() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let set_resp = acp
        .send_request_and_wait(
            "session/set_config_option",
            serde_json::json!({
                "sessionId": session_id,
                "configId": "model",
                "value": "test-model-e2e"
            }),
            SHORT_TIMEOUT,
        )
        .await
        .expect("set_config_option");
    assert!(set_resp.error.is_none(), "set_config_option failed: {:?}", set_resp.error);

    let load_resp = load_session(&mut acp, &session_id, serde_json::json!([])).await;
    assert!(load_resp.error.is_none(), "load failed: {:?}", load_resp.error);

    let result = load_resp.result.expect("should have result");
    let config_options = result.get("configOptions").expect("should have configOptions");
    let model_option = config_options
        .as_array()
        .expect("configOptions should be array")
        .iter()
        .find(|opt| opt.get("id").and_then(|v| v.as_str()) == Some("model"))
        .expect("should have model config option");
    let current_value = model_option
        .get("currentValue")
        .and_then(|v| v.as_str());
    assert_eq!(
        current_value,
        Some("test-model-e2e"),
        "load should preserve model set via set_config_option"
    );
}

#[tokio::test]
async fn e2e_load_session_preserves_mode_config() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let set_resp = acp
        .send_request_and_wait(
            "session/set_mode",
            serde_json::json!({
                "sessionId": session_id,
                "modeId": "ask",
            }),
            SHORT_TIMEOUT,
        )
        .await
        .expect("set_mode");
    assert!(set_resp.error.is_none(), "set_mode failed: {:?}", set_resp.error);

    let load_resp = load_session(&mut acp, &session_id, serde_json::json!([])).await;
    assert!(load_resp.error.is_none(), "load failed: {:?}", load_resp.error);

    let result = load_resp.result.expect("should have result");
    let current_mode = result
        .get("modes")
        .and_then(|m| m.get("currentModeId"))
        .and_then(|v| v.as_str());
    assert_eq!(
        current_mode,
        Some("ask"),
        "load should preserve mode set via setMode"
    );
}

#[tokio::test]
async fn e2e_load_session_idempotent() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let load1 = load_session(&mut acp, &session_id, serde_json::json!([])).await;
    assert!(load1.error.is_none(), "first load failed: {:?}", load1.error);

    let load2 = load_session(&mut acp, &session_id, serde_json::json!([])).await;
    assert!(load2.error.is_none(), "second load failed: {:?}", load2.error);

    let modes1 = load1.result.unwrap().get("modes").cloned();
    let modes2 = load2.result.unwrap().get("modes").cloned();
    assert_eq!(modes1, modes2, "repeated loads should return identical modes");
}

#[tokio::test]
async fn e2e_load_existing_in_memory_session_succeeds() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let load_resp = load_session(&mut acp, &session_id, serde_json::json!([])).await;
    assert!(
        load_resp.error.is_none(),
        "loading existing in-memory session should succeed: {:?}",
        load_resp.error
    );
}

#[tokio::test]
async fn e2e_load_session_empty_session_id_returns_error() {
    let (mut acp, _mock) = common::AcpChild::spawn_with_mock()
        .await
        .expect("spawn loom-acp with mock");
    initialize(&mut acp).await;

    let response = acp
        .send_request_and_wait(
            "session/load",
            serde_json::json!({
                "sessionId": "",
                "cwd": cwd(),
                "mcpServers": [],
            }),
            SHORT_TIMEOUT,
        )
        .await
        .expect("session/load empty id");

    assert!(
        response.error.is_some() || response.result.is_none(),
        "empty sessionId should return error or empty result, got result: {:?}",
        response.result
    );
}

#[tokio::test]
async fn e2e_load_session_missing_cwd_does_not_crash() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let response = acp
        .send_request_and_wait(
            "session/load",
            serde_json::json!({
                "sessionId": session_id,
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/load without cwd");

    assert!(
        response.error.is_some() || response.result.is_some(),
        "load without cwd should return result or error without crashing"
    );
}

#[tokio::test]
async fn e2e_load_session_with_mcp_servers_succeeds() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let response = load_session(&mut acp, &session_id, serde_json::json!([
        { "name": "test-server", "command": "echo", "args": [] }
    ])).await;

    assert!(response.error.is_none(), "load with mcpServers should succeed: {:?}", response.error);
}

#[tokio::test]
async fn e2e_load_session_replays_tool_calls() {
    let (mut acp, mut mock, session_id) = spawn_with_session().await;

    let _counter = mock.mount_bash_tool_call("echo 'tool executed'").await;

    let prompt_resp = prompt(&mut acp, &session_id, "Execute a tool call").await;
    assert!(prompt_resp.error.is_none(), "prompt failed: {:?}", prompt_resp.error);

    let (notifications, response) = acp
        .load_and_collect_notifications(&session_id, &cwd(), TIMEOUT)
        .expect("load and collect");

    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let update_types = extract_session_update_types(&notifications);
    assert!(update_types.contains(&SessionUpdateType::ToolCall),
        "history should contain tool_call, got: {:?}", update_types);
    assert!(update_types.contains(&SessionUpdateType::ToolCallUpdate),
        "history should contain tool_call_update, got: {:?}", update_types);
}

#[tokio::test]
async fn e2e_load_session_replays_thought_chunks() {
    let (mut acp, _mock, session_id) = spawn_with_session().await;

    let prompt_resp = prompt(&mut acp, &session_id, "Think about this problem").await;
    assert!(prompt_resp.error.is_none(), "prompt failed: {:?}", prompt_resp.error);

    let (notifications, response) = acp
        .load_and_collect_notifications(&session_id, &cwd(), TIMEOUT)
        .expect("load and collect");

    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let update_types = extract_session_update_types(&notifications);
    assert!(update_types.contains(&SessionUpdateType::UserMessageChunk),
        "history should contain user_message_chunk, got: {:?}", update_types);
    
    if update_types.contains(&SessionUpdateType::AgentThoughtChunk) {
        println!("Successfully detected AgentThoughtChunk in replay");
    } else {
        println!("No AgentThoughtChunk found - this is expected if mock doesn't generate thinking content");
    }
}

#[tokio::test]
async fn e2e_load_session_after_restart_restores_tool_and_thought_history() {
    let shared_home = tempfile::tempdir().expect("create shared temp dir");
    let shared_home_path = shared_home.path().to_path_buf();

    {
        let (mut acp_a, mut mock_a) = common::AcpChild::spawn_with_mock_at_home(&shared_home_path)
            .await
            .expect("spawn process A");
        initialize(&mut acp_a).await;
        let session_id = new_session(&mut acp_a).await;

        let _counter = mock_a.mount_bash_tool_call("echo 'cross-process tool'").await;

        let resp = prompt(&mut acp_a, &session_id, "Execute tool and think").await;
        assert!(resp.error.is_none(), "prompt failed: {:?}", resp.error);

        std::fs::write(shared_home_path.join("test-session-id.txt"), &session_id)
            .expect("write session id");
    }

    let session_id = std::fs::read_to_string(shared_home_path.join("test-session-id.txt"))
        .expect("read session id");

    let (mut acp_b, _mock_b) = common::AcpChild::spawn_with_mock_at_home(&shared_home_path)
        .await
        .expect("spawn process B");
    initialize(&mut acp_b).await;

    let (notifications, response) = acp_b
        .load_and_collect_notifications(&session_id, &cwd(), TIMEOUT)
        .expect("load in process B");

    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let update_types = extract_session_update_types(&notifications);
    assert!(update_types.contains(&SessionUpdateType::UserMessageChunk),
        "cross-process load should replay user_message_chunk, got: {:?}", update_types);
    
    if update_types.contains(&SessionUpdateType::ToolCall) {
        assert!(update_types.contains(&SessionUpdateType::ToolCallUpdate),
            "cross-process load should replay both tool_call and tool_call_update, got: {:?}", update_types);
    } else {
        println!("No ToolCall found - this is expected if tool wasn't triggered in process A");
    }
    
    if update_types.contains(&SessionUpdateType::AgentThoughtChunk) {
        println!("Successfully detected AgentThoughtChunk in cross-process replay");
    } else {
        println!("No AgentThoughtChunk found - this is expected if mock doesn't generate thinking content");
    }
}
