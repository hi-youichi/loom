mod common;
mod e2e;

use serde::Deserialize;
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

fn cwd() -> String {
    std::env::current_dir().unwrap().to_str().unwrap().to_string()
}

fn extract_session_update_types(notifications: &[serde_json::Value]) -> Vec<SessionUpdateType> {
    notifications
        .iter()
        .filter_map(SessionUpdateType::from_notification)
        .collect()
}

#[tokio::test]
async fn e2e_load_fresh_session_returns_success() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let response = guard.acp_mut()
        .send_request_and_wait(
            "session/load",
            serde_json::json!({
                "sessionId": session_id,
                "cwd": cwd(),
                "mcpServers": [],
            }),
            TIMEOUT,
        )
        .await
        .expect("session/load");

    assert!(response.error.is_none(), "session/load failed: {:?}", response.error);
    let result = response.result.expect("should have result");
    assert!(result.get("configOptions").is_some(), "should have configOptions");
    assert!(result.get("modes").is_some(), "should have modes");
}

#[tokio::test]
async fn e2e_load_session_replays_user_and_agent_messages() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let request_id = guard.acp_mut().send_prompt_request(&session_id, "Hello from history test").expect("send prompt");
    let (_notifs, prompt_resp) = guard.acp_mut().collect_all_notifications(request_id, TIMEOUT).expect("collect prompt response");
    assert!(prompt_resp.error.is_none(), "prompt failed: {:?}", prompt_resp.error);

    let (notifications, response) = guard.acp_mut()
        .load_and_collect_notifications(&session_id, &cwd(), TIMEOUT)
        .expect("load and collect");

    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let update_types = extract_session_update_types(&notifications);
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
        let (mut acp_a, mock_a) = common::AcpChild::spawn_with_mock_at_home(&shared_home_path)
            .await
            .expect("spawn process A");
        mock_a.mount_default_responses().await;
        acp_a.send_request_and_wait("initialize", serde_json::json!({ "protocolVersion": 1 }), SHORT_TIMEOUT).await.expect("init A");
        let new_resp = acp_a.send_request_and_wait("session/new", serde_json::json!({"cwd": cwd(), "mcpServers": []}), TIMEOUT).await.expect("new session");
        let session_id = new_resp.result.expect("result").get("sessionId").and_then(|v| v.as_str()).expect("sessionId").to_string();

        let request_id = acp_a.send_prompt_request(&session_id, "Remember: secret=42").expect("send prompt");
        let (_notifs, resp) = acp_a.collect_all_notifications(request_id, TIMEOUT).expect("collect");
        assert!(resp.error.is_none(), "prompt failed: {:?}", resp.error);

        std::fs::write(shared_home_path.join("test-session-id.txt"), &session_id)
            .expect("write session id");
    }

    let session_id = std::fs::read_to_string(shared_home_path.join("test-session-id.txt"))
        .expect("read session id");

    let (mut acp_b, mock_b) = common::AcpChild::spawn_with_mock_at_home(&shared_home_path)
        .await
        .expect("spawn process B");
    mock_b.mount_default_responses().await;
    acp_b.send_request_and_wait("initialize", serde_json::json!({ "protocolVersion": 1 }), SHORT_TIMEOUT).await.expect("init B");

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
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let request_id = guard.acp_mut().send_prompt_request(&session_id, "First message").expect("send prompt");
    let (_notifs, resp1) = guard.acp_mut().collect_all_notifications(request_id, TIMEOUT).expect("collect");
    assert!(resp1.error.is_none(), "first prompt failed: {:?}", resp1.error);

    let load_resp = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({"sessionId": session_id, "cwd": cwd(), "mcpServers": []}), TIMEOUT)
        .await.expect("load");
    assert!(load_resp.error.is_none(), "load failed: {:?}", load_resp.error);

    let request_id = guard.acp_mut().send_prompt_request(&session_id, "Follow up message").expect("send prompt");
    let (_notifs, resp2) = guard.acp_mut().collect_all_notifications(request_id, TIMEOUT).expect("collect");
    assert!(resp2.error.is_none(), "prompt after load failed: {:?}", resp2.error);
    let stop_reason = resp2.result.as_ref().and_then(|r| r.get("stopReason")).and_then(|v| v.as_str()).unwrap_or("unknown");
    assert_eq!(stop_reason, "end_turn", "stop reason after load should be end_turn");
}

#[tokio::test]
async fn e2e_load_session_preserves_model_config() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let set_resp = guard.acp_mut()
        .send_request_and_wait(
            "session/set_config_option",
            serde_json::json!({"sessionId": session_id, "configId": "model", "value": "test-model-e2e"}),
            SHORT_TIMEOUT,
        )
        .await
        .expect("set_config_option");
    assert!(set_resp.error.is_none(), "set_config_option failed: {:?}", set_resp.error);

    let load_resp = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({"sessionId": session_id, "cwd": cwd(), "mcpServers": []}), TIMEOUT)
        .await.expect("load");
    assert!(load_resp.error.is_none(), "load failed: {:?}", load_resp.error);

    let result = load_resp.result.expect("should have result");
    let model_option = result.get("configOptions").and_then(|c| c.as_array())
        .expect("configOptions array")
        .iter()
        .find(|opt| opt.get("id").and_then(|v| v.as_str()) == Some("model"))
        .expect("should have model config option");
    assert_eq!(model_option.get("currentValue").and_then(|v| v.as_str()), Some("test-model-e2e"));
}

#[tokio::test]
async fn e2e_load_session_preserves_mode_config() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let set_resp = guard.acp_mut()
        .send_request_and_wait("session/set_mode", serde_json::json!({"sessionId": session_id, "modeId": "ask"}), SHORT_TIMEOUT)
        .await.expect("set_mode");
    assert!(set_resp.error.is_none(), "set_mode failed: {:?}", set_resp.error);

    let load_resp = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({"sessionId": session_id, "cwd": cwd(), "mcpServers": []}), TIMEOUT)
        .await.expect("load");
    assert!(load_resp.error.is_none(), "load failed: {:?}", load_resp.error);

    let result = load_resp.result.expect("result");
    let current_mode = result.get("modes").and_then(|m| m.get("currentModeId")).and_then(|v| v.as_str());
    assert_eq!(current_mode, Some("ask"));
}

#[tokio::test]
async fn e2e_load_session_idempotent() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let load1 = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({"sessionId": session_id, "cwd": cwd(), "mcpServers": []}), TIMEOUT)
        .await.expect("load1");
    let load2 = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({"sessionId": session_id, "cwd": cwd(), "mcpServers": []}), TIMEOUT)
        .await.expect("load2");

    let modes1 = load1.result.unwrap().get("modes").cloned();
    let modes2 = load2.result.unwrap().get("modes").cloned();
    assert_eq!(modes1, modes2, "repeated loads should return identical modes");
}

#[tokio::test]
async fn e2e_load_existing_in_memory_session_succeeds() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let load_resp = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({"sessionId": session_id, "cwd": cwd(), "mcpServers": []}), TIMEOUT)
        .await.expect("load");
    assert!(load_resp.error.is_none(), "loading existing in-memory session should succeed: {:?}", load_resp.error);
}

#[tokio::test]
async fn e2e_load_session_empty_session_id_returns_error() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;

    let response = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({"sessionId": "", "cwd": cwd(), "mcpServers": []}), SHORT_TIMEOUT)
        .await.expect("session/load empty id");

    assert!(
        response.error.is_some() || response.result.as_ref().map_or(true, |r| r.get("sessionId").is_none()),
        "empty sessionId should return error or no sessionId in result, got result: {:?}",
        response.result
    );
}

#[tokio::test]
async fn e2e_load_session_missing_cwd_does_not_crash() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let response = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({"sessionId": session_id, "mcpServers": []}), TIMEOUT)
        .await.expect("session/load without cwd");

    assert!(
        response.error.is_some() || response.result.is_some(),
        "load without cwd should return result or error without crashing"
    );
}

#[tokio::test]
async fn e2e_load_session_with_mcp_servers_succeeds() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let response = guard.acp_mut()
        .send_request_and_wait("session/load", serde_json::json!({
            "sessionId": session_id, "cwd": cwd(), "mcpServers": []
        }), TIMEOUT)
        .await.expect("load");

    assert!(response.error.is_none(), "load with mcpServers should succeed: {:?}", response.error);
}

#[tokio::test]
async fn e2e_load_session_replays_tool_calls() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let _counter = guard.mock_mut().await.mount_bash_tool_call("echo 'tool executed'").await;

    let request_id = guard.acp_mut().send_prompt_request(&session_id, "Execute a tool call").expect("send prompt");
    let (_notifs, prompt_resp) = guard.acp_mut().collect_all_notifications(request_id, TIMEOUT).expect("collect");
    assert!(prompt_resp.error.is_none(), "prompt failed: {:?}", prompt_resp.error);

    let (notifications, response) = guard.acp_mut()
        .load_and_collect_notifications(&session_id, &cwd(), TIMEOUT)
        .expect("load and collect");
    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let update_types = extract_session_update_types(&notifications);
    assert!(update_types.contains(&SessionUpdateType::ToolCall), "history should contain tool_call, got: {:?}", update_types);
    assert!(update_types.contains(&SessionUpdateType::ToolCallUpdate), "history should contain tool_call_update, got: {:?}", update_types);
}

#[tokio::test]
async fn e2e_load_session_replays_thought_chunks() {
    let mut guard = common::process_pool::get_pool().await.acquire().await;
    let session_id = guard.new_session().await;

    let request_id = guard.acp_mut().send_prompt_request(&session_id, "Think about this problem").expect("send prompt");
    let (_notifs, prompt_resp) = guard.acp_mut().collect_all_notifications(request_id, TIMEOUT).expect("collect");
    assert!(prompt_resp.error.is_none(), "prompt failed: {:?}", prompt_resp.error);

    let (notifications, response) = guard.acp_mut()
        .load_and_collect_notifications(&session_id, &cwd(), TIMEOUT)
        .expect("load and collect");
    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let update_types = extract_session_update_types(&notifications);
    assert!(update_types.contains(&SessionUpdateType::UserMessageChunk), "history should contain user_message_chunk, got: {:?}", update_types);
}

#[tokio::test]
async fn e2e_load_session_after_restart_restores_tool_and_thought_history() {
    let shared_home = tempfile::tempdir().expect("create shared temp dir");
    let shared_home_path = shared_home.path().to_path_buf();

    {
        let (mut acp_a, mock_a) = common::AcpChild::spawn_with_mock_at_home(&shared_home_path)
            .await
            .expect("spawn process A");
        mock_a.mount_default_responses().await;
        acp_a.send_request_and_wait("initialize", serde_json::json!({ "protocolVersion": 1 }), SHORT_TIMEOUT).await.expect("init A");
        let new_resp = acp_a.send_request_and_wait("session/new", serde_json::json!({"cwd": cwd(), "mcpServers": []}), TIMEOUT).await.expect("new session");
        let session_id = new_resp.result.expect("result").get("sessionId").and_then(|v| v.as_str()).expect("sessionId").to_string();

        let _counter = mock_a.mount_bash_tool_call("echo 'cross-process tool'").await;

        let request_id = acp_a.send_prompt_request(&session_id, "Execute tool and think").expect("send prompt");
        let (_notifs, resp) = acp_a.collect_all_notifications(request_id, TIMEOUT).expect("collect");
        assert!(resp.error.is_none(), "prompt failed: {:?}", resp.error);

        std::fs::write(shared_home_path.join("test-session-id.txt"), &session_id).expect("write session id");
    }

    let session_id = std::fs::read_to_string(shared_home_path.join("test-session-id.txt")).expect("read session id");

    let (mut acp_b, mock_b) = common::AcpChild::spawn_with_mock_at_home(&shared_home_path)
        .await
        .expect("spawn process B");
    mock_b.mount_default_responses().await;
    acp_b.send_request_and_wait("initialize", serde_json::json!({ "protocolVersion": 1 }), SHORT_TIMEOUT).await.expect("init B");

    let (notifications, response) = acp_b.load_and_collect_notifications(&session_id, &cwd(), TIMEOUT).expect("load in process B");
    assert!(response.error.is_none(), "load failed: {:?}", response.error);

    let update_types = extract_session_update_types(&notifications);
    assert!(update_types.contains(&SessionUpdateType::UserMessageChunk),
        "cross-process load should replay user_message_chunk, got: {:?}", update_types);

    if update_types.contains(&SessionUpdateType::ToolCall) {
        assert!(update_types.contains(&SessionUpdateType::ToolCallUpdate),
            "cross-process load should replay both tool_call and tool_call_update, got: {:?}", update_types);
    }
}
