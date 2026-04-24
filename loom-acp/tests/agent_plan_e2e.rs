mod common;
#[allow(unused_imports)]
mod e2e;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use wiremock::{Mock, MockServer, Respond, ResponseTemplate};
use wiremock::matchers::{method, path};

use common::{AcpChild, PlanEntryPriority, PlanEntryStatus};

const TIMEOUT: Duration = Duration::from_secs(20);

fn todo_write_args() -> serde_json::Value {
    json!({
        "todos": [
            {"id": "1", "content": "Analyze the codebase", "status": "pending", "priority": "high"},
            {"id": "2", "content": "Implement changes", "status": "pending", "priority": "high"},
            {"id": "3", "content": "Add tests", "status": "pending", "priority": "medium"}
        ]
    })
}

fn todo_write_updated_in_progress() -> serde_json::Value {
    json!({
        "todos": [
            {"id": "1", "content": "Analyze the codebase", "status": "in_progress", "priority": "high"},
            {"id": "2", "content": "Implement changes", "status": "pending", "priority": "high"},
            {"id": "3", "content": "Add tests", "status": "pending", "priority": "medium"}
        ]
    })
}

fn todo_write_updated_completed() -> serde_json::Value {
    json!({
        "todos": [
            {"id": "1", "content": "Analyze the codebase", "status": "completed", "priority": "high"},
            {"id": "2", "content": "Implement changes", "status": "in_progress", "priority": "high"},
            {"id": "3", "content": "Add tests", "status": "pending", "priority": "medium"}
        ]
    })
}

fn streaming_tool_call_response(tool_name: &str, args: &serde_json::Value) -> String {
    let args_str = serde_json::to_string(args).unwrap_or_default();
    let content_chunk = json!({
        "id": "chatcmpl-plan",
        "object": "chat.completion.chunk",
        "created": 1234567890,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": "" },
            "finish_reason": null
        }]
    });
    let tool_call_chunk = json!({
        "id": "chatcmpl-plan",
        "object": "chat.completion.chunk",
        "created": 1234567890,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_todo_1",
                    "type": "function",
                    "function": { "name": tool_name, "arguments": args_str }
                }]
            },
            "finish_reason": null
        }]
    });
    let finish_chunk = json!({
        "id": "chatcmpl-plan",
        "object": "chat.completion.chunk",
        "created": 1234567890,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "tool_calls"
        }]
    });
    format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        content_chunk, tool_call_chunk, finish_chunk
    )
}

fn streaming_text_response(text: &str) -> String {
    let text_chunk = json!({
        "id": "chatcmpl-done",
        "object": "chat.completion.chunk",
        "created": 1234567890,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": text },
            "finish_reason": null
        }]
    });
    let finish_chunk = json!({
        "id": "chatcmpl-done",
        "object": "chat.completion.chunk",
        "created": 1234567890,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    });
    format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        text_chunk, finish_chunk
    )
}

struct PlanThenDoneResponder {
    step: Arc<AtomicUsize>,
    tool_name: String,
    tool_args: serde_json::Value,
}

impl Respond for PlanThenDoneResponder {
    fn respond(&self, _request: &wiremock::Request) -> ResponseTemplate {
        let step = self.step.fetch_add(1, Ordering::SeqCst);
        let body = if step == 0 {
            streaming_tool_call_response(&self.tool_name, &self.tool_args)
        } else {
            streaming_text_response("Done.")
        };
        ResponseTemplate::new(200)
            .set_body_raw(body.into_bytes(), "text/event-stream")
    }
}

struct MultiStepPlanResponder {
    step: Arc<AtomicUsize>,
}

impl Respond for MultiStepPlanResponder {
    fn respond(&self, _request: &wiremock::Request) -> ResponseTemplate {
        let step = self.step.fetch_add(1, Ordering::SeqCst);
        let body = match step {
            0 => streaming_tool_call_response("todo_write", &todo_write_args()),
            1 => streaming_tool_call_response("todo_write", &todo_write_updated_in_progress()),
            2 => streaming_tool_call_response("todo_write", &todo_write_updated_completed()),
            _ => streaming_text_response("Done."),
        };
        ResponseTemplate::new(200)
            .set_body_raw(body.into_bytes(), "text/event-stream")
    }
}

fn models_response() -> serde_json::Value {
    json!({
        "object": "list",
        "data": [{
            "id": "test-model",
            "object": "model",
            "created": 1234567890,
            "owned_by": "test-org"
        }]
    })
}

async fn spawn_with_plan_mock(responder: impl Respond + 'static) -> (AcpChild, MockServer) {
    spawn_with_plan_mock_and_subagent(responder, None).await
}

async fn spawn_with_plan_mock_and_subagent(
    responder: impl Respond + 'static,
    subagent_config: Option<(&str, &str)>,
) -> (AcpChild, MockServer) {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(responder)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(models_response()))
        .mount(&server)
        .await;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().to_path_buf();
    let config_toml = format!(
        r#"[default]
provider = "mock"

[[providers]]
name = "mock"
api_key = "test-key"
base_url = "{}/v1"
model = "test-model"
"#,
        server.uri()
    );
    std::fs::write(home.join("config.toml"), config_toml).expect("write config");

    if let Some((name, config_yaml)) = subagent_config {
        let agent_dir = home.join("agents").join(name);
        std::fs::create_dir_all(&agent_dir).expect("create agent dir");
        std::fs::write(agent_dir.join("config.yaml"), config_yaml).expect("write subagent config");
    }

    let acp = AcpChild::spawn_with_temp_dir(Some(&home), Some(temp_dir)).expect("spawn");
    (acp, server)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_plan_emitted_on_todo_write() {
    let (mut acp, _server) = spawn_with_plan_mock(PlanThenDoneResponder {
        step: Arc::new(AtomicUsize::new(0)),
        tool_name: "todo_write".to_string(),
        tool_args: todo_write_args(),
    }).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan to refactor the module", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);
    assert!(!plans.is_empty(), "should receive at least one plan notification");
}

#[tokio::test]
async fn e2e_plan_all_entries_pending_on_create() {
    let (mut acp, _server) = spawn_with_plan_mock(PlanThenDoneResponder {
        step: Arc::new(AtomicUsize::new(0)),
        tool_name: "todo_write".to_string(),
        tool_args: todo_write_args(),
    }).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);
    let first = plans.first().expect("should have at least one plan");
    assert!(first.entries.iter().all(|e| e.status == PlanEntryStatus::Pending),
        "all initial entries should be pending: {:?}", first.entries);
}

#[tokio::test]
async fn e2e_plan_entries_have_correct_content() {
    let (mut acp, _server) = spawn_with_plan_mock(PlanThenDoneResponder {
        step: Arc::new(AtomicUsize::new(0)),
        tool_name: "todo_write".to_string(),
        tool_args: todo_write_args(),
    }).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, _) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan", TIMEOUT)
        .expect("prompt and collect");

    let first = plans.first().expect("should have plan");
    assert_eq!(first.entries[0].content, "Analyze the codebase");
    assert_eq!(first.entries[1].content, "Implement changes");
    assert_eq!(first.entries[2].content, "Add tests");
}

#[tokio::test]
async fn e2e_plan_entries_have_correct_priority() {
    let (mut acp, _server) = spawn_with_plan_mock(PlanThenDoneResponder {
        step: Arc::new(AtomicUsize::new(0)),
        tool_name: "todo_write".to_string(),
        tool_args: todo_write_args(),
    }).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, _) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan", TIMEOUT)
        .expect("prompt and collect");

    let first = plans.first().expect("should have plan");
    assert_eq!(first.entries[0].priority, PlanEntryPriority::High);
    assert_eq!(first.entries[1].priority, PlanEntryPriority::High);
    assert_eq!(first.entries[2].priority, PlanEntryPriority::Medium);
}

#[tokio::test]
async fn e2e_plan_status_updates_on_todo_change() {
    let (mut acp, _server) = spawn_with_plan_mock(MultiStepPlanResponder {
        step: Arc::new(AtomicUsize::new(0)),
    }).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create and execute a multi-step plan", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);
    assert!(plans.len() >= 2, "should receive multiple plan updates, got {}", plans.len());

    let has_in_progress = plans.iter().any(|p| {
        p.entries.iter().any(|e| e.status == PlanEntryStatus::InProgress)
    });
    assert!(has_in_progress, "at least one entry should be in_progress across plan updates");
}

#[tokio::test]
async fn e2e_plan_completed_status_correct() {
    let (mut acp, _server) = spawn_with_plan_mock(MultiStepPlanResponder {
        step: Arc::new(AtomicUsize::new(0)),
    }).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, _) = acp
        .prompt_and_collect_plans(&session_id, "Create and execute a multi-step plan", TIMEOUT)
        .expect("prompt and collect");

    let has_completed = plans.iter().any(|p| {
        p.entries.iter().any(|e| e.status == PlanEntryStatus::Completed)
    });
    assert!(has_completed, "at least one entry should be completed across plan updates");
}

#[tokio::test]
async fn e2e_plan_full_replacement_semantics() {
    let (mut acp, _server) = spawn_with_plan_mock(MultiStepPlanResponder {
        step: Arc::new(AtomicUsize::new(0)),
    }).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, _) = acp
        .prompt_and_collect_plans(&session_id, "Create and execute a multi-step plan", TIMEOUT)
        .expect("prompt and collect");

    for plan in &plans {
        assert!(!plan.entries.is_empty(), "each plan should contain full entries list");
        assert_eq!(plan.session_update, "plan");
    }
}

#[tokio::test]
async fn e2e_plan_not_emitted_without_todo_write() {
    let server = MockServer::start().await;

    let body = streaming_text_response("Hello! I'll help you with that.");
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_raw(body.into_bytes(), "text/event-stream"))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(models_response()))
        .mount(&server)
        .await;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().to_path_buf();
    let config_toml = format!(
        r#"[default]
provider = "mock"

[[providers]]
name = "mock"
api_key = "test-key"
base_url = "{}/v1"
model = "test-model"
"#,
        server.uri()
    );
    std::fs::write(home.join("config.toml"), config_toml).expect("write config");

    let mut acp = AcpChild::spawn_with_temp_dir(Some(&home), Some(temp_dir)).expect("spawn");
    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Say hello", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);
    assert!(plans.is_empty(), "should not receive plan notification when LLM doesn't call todo_write");
}

#[tokio::test]
async fn e2e_plan_emitted_alongside_tool_update() {
    let (mut acp, _server) = spawn_with_plan_mock(PlanThenDoneResponder {
        step: Arc::new(AtomicUsize::new(0)),
        tool_name: "todo_write".to_string(),
        tool_args: todo_write_args(),
    }).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let request_id = acp.send_prompt_request(&session_id, "Create a plan").expect("send prompt");
    let (notifications, response) = acp.collect_all_notifications(request_id, TIMEOUT).expect("collect");

    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);

    let has_tool_call = notifications.iter().any(|n: &serde_json::Value| {
        n.pointer("/params/update/sessionUpdate").and_then(|v: &serde_json::Value| v.as_str()) == Some("tool_call")
    });
    let has_tool_update = notifications.iter().any(|n: &serde_json::Value| {
        n.pointer("/params/update/sessionUpdate").and_then(|v: &serde_json::Value| v.as_str()) == Some("tool_call_update")
    });
    let has_plan = notifications.iter().any(|n: &serde_json::Value| {
        n.pointer("/params/update/sessionUpdate").and_then(|v: &serde_json::Value| v.as_str()) == Some("plan")
    });

    assert!(has_tool_call, "should emit tool_call notification");
    assert!(has_tool_update, "should emit tool_call_update notification");
    assert!(has_plan, "should emit plan notification");
}

// ---------------------------------------------------------------------------
// Sub-agent plan propagation tests
// ---------------------------------------------------------------------------

fn invoke_agent_args(agent: &str, task: &str) -> serde_json::Value {
    json!({
        "agents": [
            {"agent": agent, "task": task}
        ]
    })
}

struct SubAgentPlanResponder {
    step: Arc<AtomicUsize>,
}

impl Respond for SubAgentPlanResponder {
    fn respond(&self, _request: &wiremock::Request) -> ResponseTemplate {
        let step = self.step.fetch_add(1, Ordering::SeqCst);
        let body = match step {
            0 => streaming_tool_call_response(
                "invoke_agent",
                &invoke_agent_args("plan-sub", "Create a plan to analyze the project"),
            ),
            1 => streaming_tool_call_response("todo_write", &todo_write_args()),
            2 => streaming_text_response("Plan created successfully."),
            _ => streaming_text_response("Done."),
        };
        ResponseTemplate::new(200)
            .set_body_raw(body.into_bytes(), "text/event-stream")
    }
}

/// TODO: Sub-agent invoke_agent tool events (ToolCall/ToolEnd) currently do not propagate
/// through the any_stream_event_sender path to the ACP client. The plan protocol conversion
/// works correctly at the SessionNotifier level (verified in plan_bridge_test.rs), but the
/// sub-agent's ReactRunner does not emit these events via the on_event callback.
/// Tracked as a known issue. Once fixed, remove #[ignore].
#[tokio::test]
#[ignore]
async fn e2e_subagent_plan_propagates_to_client() {
    let subagent_yaml = "name: plan-sub\ndescription: Test sub-agent for plan propagation\n";
    let (mut acp, _server) = spawn_with_plan_mock_and_subagent(
        SubAgentPlanResponder {
            step: Arc::new(AtomicUsize::new(0)),
        },
        Some(("plan-sub", subagent_yaml)),
    ).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let request_id = acp
        .send_prompt_request(&session_id, "Use a sub-agent to create a plan")
        .expect("send prompt");
    let (notifications, response) = acp
        .collect_all_notifications(request_id, TIMEOUT)
        .expect("collect");

    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);

    let has_invoke = notifications.iter().any(|n: &serde_json::Value| {
        n.pointer("/params/update/rawInput/agents/0/agent")
            .and_then(|v| v.as_str())
            == Some("plan-sub")
    });
    assert!(has_invoke, "should emit invoke_agent tool_call notification");

    let has_plan = notifications.iter().any(|n: &serde_json::Value| {
        n.pointer("/params/update/sessionUpdate")
            .and_then(|v: &serde_json::Value| v.as_str())
            == Some("plan")
    });
    assert!(
        has_plan,
        "should emit plan notification from sub-agent's todo_write"
    );
}

/// TODO: See e2e_subagent_plan_propagates_to_client — sub-agent events not yet propagating.
#[tokio::test]
#[ignore]
async fn e2e_subagent_plan_entries_have_correct_content() {
    let subagent_yaml = "name: plan-sub\ndescription: Test sub-agent for plan propagation\n";
    let (mut acp, _server) = spawn_with_plan_mock_and_subagent(
        SubAgentPlanResponder {
            step: Arc::new(AtomicUsize::new(0)),
        },
        Some(("plan-sub", subagent_yaml)),
    ).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, response) = acp
        .prompt_and_collect_plans(
            &session_id,
            "Use a sub-agent to create a plan",
            TIMEOUT,
        )
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);
    let first = plans.first().expect("should have at least one plan from sub-agent");
    assert_eq!(first.entries.len(), 3, "should have 3 plan entries");
    assert_eq!(first.entries[0].content, "Analyze the codebase");
    assert_eq!(first.entries[0].priority, PlanEntryPriority::High);
    assert_eq!(first.entries[0].status, PlanEntryStatus::Pending);
    assert_eq!(first.entries[1].content, "Implement changes");
    assert_eq!(first.entries[2].content, "Add tests");
    assert_eq!(first.entries[2].priority, PlanEntryPriority::Medium);
}

struct ParentAndSubAgentPlanResponder {
    step: Arc<AtomicUsize>,
}

impl Respond for ParentAndSubAgentPlanResponder {
    fn respond(&self, _request: &wiremock::Request) -> ResponseTemplate {
        let step = self.step.fetch_add(1, Ordering::SeqCst);
        let body = match step {
            0 => streaming_tool_call_response(
                "invoke_agent",
                &invoke_agent_args("plan-sub", "Create a sub-plan"),
            ),
            1 => streaming_tool_call_response("todo_write", &todo_write_args()),
            2 => streaming_text_response("Sub-agent plan done."),
            3 => streaming_tool_call_response("todo_write", &json!({
                "todos": [
                    {"id": "1", "content": "Parent task 1", "status": "pending", "priority": "high"}
                ]
            })),
            _ => streaming_text_response("All done."),
        };
        ResponseTemplate::new(200)
            .set_body_raw(body.into_bytes(), "text/event-stream")
    }
}

/// TODO: See e2e_subagent_plan_propagates_to_client — sub-agent events not yet propagating.
#[tokio::test]
#[ignore]
async fn e2e_subagent_and_parent_both_emit_plans() {
    let subagent_yaml = "name: plan-sub\ndescription: Test sub-agent for plan propagation\n";
    let (mut acp, _server) = spawn_with_plan_mock_and_subagent(
        ParentAndSubAgentPlanResponder {
            step: Arc::new(AtomicUsize::new(0)),
        },
        Some(("plan-sub", subagent_yaml)),
    ).await;

    let session_id = acp.handshake(TIMEOUT).await.expect("handshake");
    let (plans, response) = acp
        .prompt_and_collect_plans(
            &session_id,
            "Create plans at both parent and sub-agent level",
            TIMEOUT,
        )
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt failed: {:?}", response.error);
    assert!(
        plans.len() >= 2,
        "should receive plans from both sub-agent and parent, got {}",
        plans.len()
    );

    let sub_plan_entries: Vec<&str> = plans
        .iter()
        .flat_map(|p| p.entries.iter().map(|e| e.content.as_str()))
        .collect();
    assert!(
        sub_plan_entries.iter().any(|c| *c == "Analyze the codebase"),
        "should include sub-agent plan entry"
    );
    assert!(
        sub_plan_entries.iter().any(|c| *c == "Parent task 1"),
        "should include parent plan entry"
    );
}
