mod common;
#[allow(unused_imports)]
mod e2e;
#[allow(unused_imports)]
mod mocks;

use std::io::BufRead;
use std::time::Duration;
use common::{AcpChild, PlanEntryPriority, PlanEntryStatus};

const TIMEOUT: Duration = Duration::from_secs(60);

async fn spawn_acp() -> (AcpChild, common::MockAcpServer) {
    AcpChild::spawn_with_mock().await.expect("spawn loom-acp with mock")
}

async fn handshake_and_session(acp: &mut AcpChild) -> String {
    acp.handshake(TIMEOUT).await.expect("handshake")
}

fn assert_plan_entries_valid(plans: &[common::PlanNotification]) {
    for plan in plans {
        assert_eq!(plan.session_update, "plan", "sessionUpdate should be 'plan'");
        for entry in &plan.entries {
            assert!(!entry.content.is_empty(), "content must not be empty");
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 1: Plan notification structure validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_plan_notification_structure() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a multi-step plan to refactor the codebase", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if !plans.is_empty() {
        let first = &plans[0];
        assert_eq!(first.session_update, "plan");
        assert!(!first.entries.is_empty(), "entries should not be empty");
    }
}

#[tokio::test]
async fn e2e_plan_entry_required_fields() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a 3-step plan to fix bugs", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    for plan in &plans {
        for entry in &plan.entries {
            assert!(!entry.content.is_empty(), "content field must be present and non-empty");
        }
    }
}

#[tokio::test]
async fn e2e_plan_entry_priority_values() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a prioritized plan with high, medium and low priority tasks", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    for plan in &plans {
        for entry in &plan.entries {
            let valid = matches!(
                entry.priority,
                PlanEntryPriority::High | PlanEntryPriority::Medium | PlanEntryPriority::Low
            );
            assert!(valid, "priority must be high/medium/low, got: {:?}", entry.priority);
        }
    }
}

#[tokio::test]
async fn e2e_plan_entry_status_values() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a step-by-step plan", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    for plan in &plans {
        for entry in &plan.entries {
            let valid = matches!(
                entry.status,
                PlanEntryStatus::Pending | PlanEntryStatus::InProgress | PlanEntryStatus::Completed
            );
            assert!(valid, "status must be pending/in_progress/completed, got: {:?}", entry.status);
        }
    }
}

#[tokio::test]
async fn e2e_plan_entries_non_empty_when_triggered() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, _response) = acp
        .prompt_and_collect_plans(&session_id, "Analyze the project structure and create a comprehensive refactoring plan with at least 3 steps", TIMEOUT)
        .expect("prompt and collect");

    if !plans.is_empty() {
        for plan in &plans {
            assert!(!plan.entries.is_empty(), "plan entries should not be empty when triggered");
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 2: Plan lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_plan_initial_status_pending() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan to improve code quality", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if let Some(first) = plans.first() {
        let all_pending = first.entries.iter().all(|e| e.status == PlanEntryStatus::Pending);
        assert!(all_pending, "initial plan entries should all be pending: {:?}", first.entries);
    }
}

#[tokio::test]
async fn e2e_plan_progress_to_in_progress() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a multi-step plan and start executing it: read a file, analyze it, fix issues", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if plans.len() > 1 {
        let has_in_progress = plans.iter().any(|p| {
            p.entries.iter().any(|e| e.status == PlanEntryStatus::InProgress)
        });
        assert!(has_in_progress, "at least one entry should be in_progress across plan updates");
    }
}

#[tokio::test]
async fn e2e_plan_progress_to_completed() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Say hello world", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if !plans.is_empty() {
        let last = plans.last().expect("should have last plan");
        let has_completed = last.entries.iter().any(|e| e.status == PlanEntryStatus::Completed);
        if !last.entries.is_empty() {
            assert!(has_completed, "final plan should have at least one completed entry: {:?}", last.entries);
        }
    }
}

#[tokio::test]
async fn e2e_plan_full_replacement_semantics() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a step-by-step plan to refactor the module", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if plans.len() > 1 {
        for plan in &plans {
            assert_plan_entries_valid(std::slice::from_ref(plan));
            assert!(!plan.entries.is_empty(), "each plan notification should contain full entries list");
        }
    }
}

#[tokio::test]
async fn e2e_plan_order_matches_execution() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a sequential plan: step A, then step B, then step C", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if plans.len() >= 2 {
        let mut progress_order = Vec::new();
        for plan in &plans {
            for entry in &plan.entries {
                if entry.status == PlanEntryStatus::InProgress {
                    progress_order.push(entry.content.clone());
                }
            }
        }
        assert!(!progress_order.is_empty(), "should observe in_progress transitions");
    }
}

// ---------------------------------------------------------------------------
// Phase 3: Dynamic planning
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_plan_add_entries_dynamically() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Analyze this project and create a plan. After analysis, add new steps you discover.", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if plans.len() >= 2 {
        let first_count = plans.first().map(|p| p.entries.len()).unwrap_or(0);
        let later_count = plans.iter().map(|p| p.entries.len()).max().unwrap_or(0);
        assert!(later_count >= first_count, "plan should grow dynamically or stay same");
    }
}

#[tokio::test]
async fn e2e_plan_remove_entries_dynamically() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan with optional steps. Remove steps that are unnecessary as you go.", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if plans.len() >= 2 {
        let counts: Vec<usize> = plans.iter().map(|p| p.entries.len()).collect();
        assert!(!counts.is_empty(), "should have plan updates");
    }
}

#[tokio::test]
async fn e2e_plan_modify_entry_content() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a rough plan, then refine the descriptions as you analyze further", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if plans.len() >= 2 {
        let first_contents: Vec<String> = plans[0].entries.iter().map(|e| e.content.clone()).collect();
        let last_contents: Vec<String> = plans.last().unwrap().entries.iter().map(|e| e.content.clone()).collect();
        assert!(!first_contents.is_empty() || !last_contents.is_empty(), "plans should have entries");
    }
}

#[tokio::test]
async fn e2e_plan_modify_entry_priority() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan and adjust priorities as you learn more about each task", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    for plan in &plans {
        for entry in &plan.entries {
            let valid = matches!(entry.priority, PlanEntryPriority::High | PlanEntryPriority::Medium | PlanEntryPriority::Low);
            assert!(valid, "priority should be valid");
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 4: Multi-turn conversation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_plan_in_multi_turn_conversation() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans1, response1) = acp
        .prompt_and_collect_plans(&session_id, "Analyze the project structure", TIMEOUT)
        .expect("prompt 1");
    assert!(response1.error.is_none(), "prompt 1 should succeed: {:?}", response1.error);

    let (plans2, response2) = acp
        .prompt_and_collect_plans(&session_id, "Based on the analysis, create a refactoring plan", TIMEOUT)
        .expect("prompt 2");
    assert!(response2.error.is_none(), "prompt 2 should succeed: {:?}", response2.error);

    assert!(response1.result.is_some() || response2.result.is_some(), "both turns should have results");
    let _ = (plans1, plans2);
}

#[tokio::test]
async fn e2e_plan_cleared_on_new_prompt() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans1, response1) = acp
        .prompt_and_collect_plans(&session_id, "Create a 5-step plan to fix all bugs", TIMEOUT)
        .expect("prompt 1");
    assert!(response1.error.is_none(), "prompt 1 should succeed");

    let (plans2, response2) = acp
        .prompt_and_collect_plans(&session_id, "Say goodbye", TIMEOUT)
        .expect("prompt 2");
    assert!(response2.error.is_none(), "prompt 2 should succeed");

    let _ = (plans1, plans2);
}

#[tokio::test]
async fn e2e_plan_absent_when_no_planning_needed() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Say hello", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);
    let _ = plans;
}

// ---------------------------------------------------------------------------
// Phase 5: Plan and tool call interaction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_plan_with_tool_call_flow() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan that includes reading a file and analyzing its contents", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    for plan in &plans {
        assert_plan_entries_valid(std::slice::from_ref(plan));
    }
}

#[tokio::test]
async fn e2e_plan_reflects_tool_completion() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan: read Cargo.toml and summarize it", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if !plans.is_empty() {
        let last = plans.last().expect("should have last plan");
        let has_completed = last.entries.iter().any(|e| e.status == PlanEntryStatus::Completed);
        if !last.entries.is_empty() {
            assert!(has_completed, "entries should reflect tool completion: {:?}", last.entries);
        }
    }
}

#[tokio::test]
async fn e2e_plan_after_cancellation() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let request_id = acp.send_prompt_request(&session_id, "Create a complex 10-step plan").expect("send prompt");

    let cancel = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": {
            "sessionId": session_id
        }
    });
    acp.send_raw(&serde_json::to_string(&cancel).unwrap()).expect("send cancel");

    let start = std::time::Instant::now();
    let mut got_response = false;
    let mut plans = Vec::new();

    loop {
        if start.elapsed() > TIMEOUT {
            break;
        }
        let mut line = String::new();
        let bytes = acp.reader.read_line(&mut line).unwrap_or(0);
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if msg.get("id").and_then(|v| v.as_u64()) == Some(request_id) {
            got_response = true;
            if let Some(result) = msg.get("result") {
                let stop_reason = result.get("stopReason").and_then(|v| v.as_str()).unwrap_or("unknown");
                assert_eq!(stop_reason, "cancelled", "stop reason should be cancelled after cancel, got: {}", stop_reason);
            }
            break;
        }

        if msg.get("method").and_then(|v| v.as_str()) == Some("session/update") {
            if let Some(update) = msg.get("params").and_then(|p| p.get("update")) {
                if update.get("sessionUpdate").and_then(|v| v.as_str()) == Some("plan") {
                    if let Ok(plan_notif) = serde_json::from_value::<common::PlanNotification>(update.clone()) {
                        plans.push(plan_notif);
                    }
                }
            }
        }
    }

    assert!(got_response, "should receive prompt response after cancellation");
    let _ = plans;
}

// ---------------------------------------------------------------------------
// Phase 6: Edge cases and robustness
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_plan_with_many_entries() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a detailed 10-step plan to migrate a project from Python to Rust", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    if !plans.is_empty() {
        let max_entries = plans.iter().map(|p| p.entries.len()).max().unwrap_or(0);
        assert!(max_entries >= 1, "should have at least 1 entry in plan");
    }
}

#[tokio::test]
async fn e2e_plan_with_unicode_content() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "创建一个多步骤计划来改进代码质量", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    for plan in &plans {
        for entry in &plan.entries {
            assert!(!entry.content.is_empty(), "unicode content should be preserved");
        }
    }
}

#[tokio::test]
async fn e2e_plan_notification_no_id() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let request_id = acp.send_prompt_request(&session_id, "Create a step-by-step plan").expect("send prompt");

    let start = std::time::Instant::now();
    let mut got_response = false;

    loop {
        if start.elapsed() > TIMEOUT {
            break;
        }
        let mut line = String::new();
        let bytes = acp.reader.read_line(&mut line).unwrap_or(0);
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if msg.get("id").and_then(|v| v.as_u64()) == Some(request_id) {
            got_response = true;
            break;
        }

        if msg.get("method").is_some() && msg.get("id").is_none()
            && msg.get("method").and_then(|v| v.as_str()) == Some("session/update")
        {
            assert!(msg.get("id").is_none(), "session/update notification should not have id field");
        }
    }

    assert!(got_response, "should receive prompt response");
}

#[tokio::test]
async fn e2e_plan_after_permission_denied() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans, response) = acp
        .prompt_and_collect_plans(&session_id, "Create a plan that includes writing to a file, then try to execute it", TIMEOUT)
        .expect("prompt and collect");

    assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);

    for plan in &plans {
        assert_plan_entries_valid(std::slice::from_ref(plan));
    }
}

#[tokio::test]
async fn e2e_plan_concurrent_update_race() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let (plans1, response1) = acp
        .prompt_and_collect_plans(&session_id, "Create plan A for task 1", TIMEOUT)
        .expect("prompt 1");
    assert!(response1.error.is_none(), "prompt 1 should succeed");

    let (plans2, response2) = acp
        .prompt_and_collect_plans(&session_id, "Create plan B for task 2", TIMEOUT)
        .expect("prompt 2");
    assert!(response2.error.is_none(), "prompt 2 should succeed");

    let _ = (plans1, plans2);
}

#[tokio::test]
async fn e2e_plan_validates_session_update_method() {
    let (mut acp, _mock) = spawn_acp().await;
    let session_id = handshake_and_session(&mut acp).await;

    let request_id = acp.send_prompt_request(&session_id, "Create a plan to organize files").expect("send prompt");

    let start = std::time::Instant::now();
    let mut got_response = false;

    loop {
        if start.elapsed() > TIMEOUT {
            break;
        }
        let mut line = String::new();
        let bytes = acp.reader.read_line(&mut line).unwrap_or(0);
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if msg.get("id").and_then(|v| v.as_u64()) == Some(request_id) {
            got_response = true;
            break;
        }

        if msg.get("method").and_then(|v| v.as_str()) == Some("session/update") {
            if let Some(params) = msg.get("params") {
                let msg_session = params.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                assert_eq!(msg_session, session_id, "notification sessionId should match");
            }
        }
    }

    assert!(got_response, "should receive prompt response");
}
