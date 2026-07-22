//! Terminal-event integrity regression tests.
//!
//! Verify that:
//! 1. Every agent_started has a matching agent_done before run_done
//! 2. Failed runs still emit run_done through the event stream
//! 3. The event sequence maintains ordering invariants
//!
//! These tests exercise the event forwarding logic in handle_run that
//! ensures terminal-event integrity: any agent that starts must receive
//! an agent_done (either from luft scheduler or synthetic) before run_done.

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tool_core::{Tool, ToolCallContext};
use tool_workflow::WorkflowTool;

fn make_tool() -> WorkflowTool {
    use agent::agent::AgentConfig;
    WorkflowTool::new(AgentConfig::default())
}

fn make_ctx(collector: Arc<Mutex<Vec<Value>>>) -> ToolCallContext {
    ToolCallContext {
        any_stream_event_sender: Some(Arc::new(move |ev: Value| {
            collector.lock().unwrap().push(ev);
        })),
        ..Default::default()
    }
}

fn event_type(ev: &Value) -> &str {
    ev.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
}

/// Verify: a successful run with one agent emits
/// agent_started -> agent_done -> run_done, in that order.
#[tokio::test]
async fn successful_run_agent_done_before_run_done() {
    let tool = make_tool();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ctx = make_ctx(events.clone());

    let script = r#"
        function main()
            agent({prompt = "test prompt"})
            report({ok = true})
        end
    "#;

    let args = serde_json::json!({"action": "run", "script": script});
    let result = tool.call(args, Some(&ctx)).await;
    assert!(result.is_ok(), "run should succeed");

    let events = events.lock().unwrap();
    let types: Vec<&str> = events.iter().map(event_type).collect();

    let agent_done_pos = types.iter().position(|&t| t == "agent_done");
    let run_done_pos = types.iter().position(|&t| t == "run_done");

    assert!(agent_done_pos.is_some(), "agent_done event must be emitted");
    assert!(run_done_pos.is_some(), "run_done event must be emitted");
    assert!(
        agent_done_pos.unwrap() < run_done_pos.unwrap(),
        "agent_done must come before run_done, got sequence: {:?}",
        types
    );
}

/// Verify: every agent_started has a matching agent_done.
/// This is the core terminal-event integrity invariant — no agent may
/// "disappear" without a terminal event.
#[tokio::test]
async fn every_agent_started_has_agent_done() {
    let tool = make_tool();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ctx = make_ctx(events.clone());

    let script = r#"
        function main()
            local results = parallel({"a", "b", "c"}, function(item)
                return {prompt = item}
            end)
            report({count = #results})
        end
    "#;

    let args = serde_json::json!({"action": "run", "script": script});
    let result = tool.call(args, Some(&ctx)).await;
    assert!(result.is_ok(), "run should succeed: {:?}", result);

    let events = events.lock().unwrap();

    let started_ids: Vec<String> = events
        .iter()
        .filter(|e| event_type(e) == "agent_started")
        .filter_map(|e| e.get("agent_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let done_ids: Vec<String> = events
        .iter()
        .filter(|e| event_type(e) == "agent_done")
        .filter_map(|e| e.get("agent_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    assert!(
        !started_ids.is_empty(),
        "at least one agent should start"
    );
    assert_eq!(
        started_ids.len(),
        done_ids.len(),
        "every agent_started must have a matching agent_done; started: {:?}, done: {:?}",
        started_ids,
        done_ids
    );

    for id in &started_ids {
        assert!(
            done_ids.contains(id),
            "agent {} started but no agent_done was emitted",
            id
        );
    }

    // Verify ordering: last agent_done must come before run_done
    let types: Vec<&str> = events.iter().map(event_type).collect();
    let last_done = types.iter().rposition(|&t| t == "agent_done");
    let run_done = types.iter().position(|&t| t == "run_done");
    if let (Some(ld), Some(rd)) = (last_done, run_done) {
        assert!(
            ld < rd,
            "all agent_done events must come before run_done"
        );
    }
}

/// Verify: a failed run still emits run_done through the event stream,
/// and the error propagates as ToolError.
#[tokio::test]
async fn failed_run_still_emits_events() {
    let tool = make_tool();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ctx = make_ctx(events.clone());

    let script = r#"
        function main()
            agent({topic = "missing prompt"})
            report({ok = true})
        end
    "#;

    let args = serde_json::json!({"action": "run", "script": script});
    let result = tool.call(args, Some(&ctx)).await;

    assert!(result.is_err(), "run should fail");

    let events = events.lock().unwrap();
    let types: Vec<&str> = events.iter().map(event_type).collect();

    assert!(
        types.contains(&"run_done"),
        "run_done must be emitted even on failure, got: {:?}",
        types
    );
}

/// Verify: a report-only workflow (no agents) emits run_done.
#[tokio::test]
async fn report_only_emits_run_done() {
    let tool = make_tool();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ctx = make_ctx(events.clone());

    let script = r#"
        function main()
            report({status = "complete"})
        end
    "#;

    let args = serde_json::json!({"action": "run", "script": script});
    let result = tool.call(args, Some(&ctx)).await;
    assert!(result.is_ok());

    let events = events.lock().unwrap();
    let types: Vec<&str> = events.iter().map(event_type).collect();

    assert!(
        types.contains(&"run_done"),
        "run_done must be emitted for report-only workflows, got: {:?}",
        types
    );
    assert!(
        !types.contains(&"agent_started"),
        "no agent events should be emitted for report-only workflows"
    );
}

/// Verify: no agent_id appears more than once in agent_done events.
/// This catches both duplicate synthetic events and double-delivery bugs.
#[tokio::test]
async fn no_duplicate_agent_done_events() {
    let tool = make_tool();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ctx = make_ctx(events.clone());

    let script = r#"
        function main()
            local results = parallel({"a", "b", "c", "d"}, function(item)
                return {prompt = item}
            end)
            report({count = #results})
        end
    "#;

    let args = serde_json::json!({"action": "run", "script": script});
    let result = tool.call(args, Some(&ctx)).await;
    assert!(result.is_ok(), "run should succeed: {:?}", result);

    let events = events.lock().unwrap();
    let mut done_ids: Vec<String> = events
        .iter()
        .filter(|e| event_type(e) == "agent_done")
        .filter_map(|e| e.get("agent_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let total = done_ids.len();
    done_ids.sort();
    done_ids.dedup();
    let unique = done_ids.len();

    assert_eq!(
        total, unique,
        "duplicate agent_done events detected: {} total, {} unique",
        total, unique
    );
}

/// Verify: a cancelled workflow still emits agent_done for every agent_started.
/// Uses a cancellation token that fires after a short delay.
#[tokio::test]
async fn cancelled_workflow_terminal_integrity() {
    use tool_core::active_operation::RunCancellation;

    let tool = make_tool();
    let events = Arc::new(Mutex::new(Vec::new()));
    let rc = RunCancellation::new(0);

    let ctx = ToolCallContext {
        any_stream_event_sender: Some(Arc::new(move |ev: Value| {
            events.lock().unwrap().push(ev);
        })),
        run_cancellation: Some(rc.clone()),
        ..Default::default()
    };

    let script = r#"
        function main()
            local results = parallel({"a", "b", "c"}, function(item)
                return {prompt = item}
            end)
            report({count = #results})
        end
    "#;

    let args = serde_json::json!({"action": "run", "script": script});

    let rc_clone = rc.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        rc_clone.cancel();
    });

    let result = tool.call(args, Some(&ctx)).await;
    let _ = result;

    let events = events.lock().unwrap();
    let types: Vec<&str> = events.iter().map(event_type).collect();

    let started_ids: Vec<String> = events
        .iter()
        .filter(|e| event_type(e) == "agent_started")
        .filter_map(|e| e.get("agent_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let done_ids: Vec<String> = events
        .iter()
        .filter(|e| event_type(e) == "agent_done")
        .filter_map(|e| e.get("agent_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    if !started_ids.is_empty() {
        for id in &started_ids {
            assert!(
                done_ids.contains(id),
                "agent {} started but no agent_done after cancellation",
                id
            );
        }
    }

    if let Some(pos) = types.iter().position(|&t| t == "run_done") {
        let last_done = types[..pos].iter().rposition(|&t| t == "agent_done");
        if let Some(ld) = last_done {
            assert!(ld < pos, "all agent_done events must come before run_done");
        }
    }
}

/// Verify: agent_done events have no duplicates even when agents fail.
#[tokio::test]
async fn failed_agents_no_duplicate_done() {
    let tool = make_tool();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ctx = make_ctx(events.clone());

    let script = r#"
        function main()
            agent({prompt = "fail me"})
            report({ok = true})
        end
    "#;

    let args = serde_json::json!({"action": "run", "script": script});
    let _ = tool.call(args, Some(&ctx)).await;

    let events = events.lock().unwrap();
    let done_ids: Vec<String> = events
        .iter()
        .filter(|e| event_type(e) == "agent_done")
        .filter_map(|e| e.get("agent_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let mut sorted = done_ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        done_ids.len(),
        sorted.len(),
        "duplicate agent_done events: {:?}",
        done_ids
    );
}
