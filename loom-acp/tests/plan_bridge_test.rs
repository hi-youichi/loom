use agent_client_protocol::{PlanEntry, PlanEntryPriority, PlanEntryStatus};

use loom_acp::stream_bridge::{loom_event_to_updates, StreamUpdate};
use loom::{AnyStreamEvent, StreamEvent};
use loom::state::ReActState;

fn make_tool_end(name: &str, result: &str) -> StreamEvent<ReActState> {
    StreamEvent::ToolEnd {
        call_id: Some("call_test".to_string()),
        name: name.to_string(),
        result: result.to_string(),
        is_error: false,
        raw_result: None,
    }
}

fn find_plan_updates(updates: &[StreamUpdate]) -> Vec<&Vec<PlanEntry>> {
    updates.iter().filter_map(|u| match u {
        StreamUpdate::Plan { entries } => Some(entries),
        _ => None,
    }).collect()
}

fn todo_result_json() -> &'static str {
    r#"3 todos
[
  { "id": "1", "content": "Analyze the codebase", "status": "pending", "priority": "high" },
  { "id": "2", "content": "Implement changes", "status": "pending", "priority": "high" },
  { "id": "3", "content": "Add tests", "status": "pending", "priority": "medium" }
]"#
}

fn todo_result_quoted_json() -> String {
    let inner = todo_result_json();
    serde_json::to_string(inner).unwrap()
}

#[test]
fn plan_emitted_on_todo_write_tool_end() {
    let ev = make_tool_end("todo_write", todo_result_json());
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].len(), 3);
}

#[test]
fn plan_all_entries_pending_on_create() {
    let ev = make_tool_end("todo_write", todo_result_json());
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    let entries = plans[0];
    assert!(entries.iter().all(|e| e.status == PlanEntryStatus::Pending));
}

#[test]
fn plan_entries_have_correct_content() {
    let ev = make_tool_end("todo_write", todo_result_json());
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    let entries = plans[0];
    assert_eq!(entries[0].content, "Analyze the codebase");
    assert_eq!(entries[1].content, "Implement changes");
    assert_eq!(entries[2].content, "Add tests");
}

#[test]
fn plan_entries_have_correct_priority() {
    let ev = make_tool_end("todo_write", todo_result_json());
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    let entries = plans[0];
    assert_eq!(entries[0].priority, PlanEntryPriority::High);
    assert_eq!(entries[1].priority, PlanEntryPriority::High);
    assert_eq!(entries[2].priority, PlanEntryPriority::Medium);
}

#[test]
fn plan_status_updates_on_todo_change() {
    let updated = r#"3 todos
[
  { "id": "1", "content": "Analyze the codebase", "status": "completed", "priority": "high" },
  { "id": "2", "content": "Implement changes", "status": "in_progress", "priority": "high" },
  { "id": "3", "content": "Add tests", "status": "pending", "priority": "medium" }
]"#;
    let ev = make_tool_end("todo_write", updated);
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    let entries = plans[0];
    assert_eq!(entries[0].status, PlanEntryStatus::Completed);
    assert_eq!(entries[1].status, PlanEntryStatus::InProgress);
    assert_eq!(entries[2].status, PlanEntryStatus::Pending);
}

#[test]
fn plan_completed_and_cancelled_map_to_completed() {
    let result = r#"2 todos
[
  { "id": "1", "content": "Task A", "status": "completed", "priority": "high" },
  { "id": "2", "content": "Task B", "status": "cancelled", "priority": "low" }
]"#;
    let ev = make_tool_end("todo_write", result);
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    let entries = plans[0];
    assert_eq!(entries[0].status, PlanEntryStatus::Completed);
    assert_eq!(entries[1].status, PlanEntryStatus::Completed);
}

#[test]
fn plan_full_replacement() {
    let updated = r#"4 todos
[
  { "id": "1", "content": "A", "status": "completed", "priority": "high" },
  { "id": "2", "content": "B", "status": "in_progress", "priority": "high" },
  { "id": "3", "content": "C", "status": "pending", "priority": "medium" },
  { "id": "4", "content": "D", "status": "pending", "priority": "low" }
]"#;
    let ev = make_tool_end("todo_write", updated);
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    assert_eq!(plans[0].len(), 4);
}

#[test]
fn plan_not_emitted_for_other_tools() {
    let ev = make_tool_end("read_file", "file contents here");
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    assert!(plans.is_empty());
}

#[test]
fn plan_emitted_alongside_tool_update() {
    let ev = make_tool_end("todo_write", todo_result_json());
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let has_tool_update = updates.iter().any(|u| matches!(u, StreamUpdate::ToolCallUpdated { .. }));
    let has_plan = updates.iter().any(|u| matches!(u, StreamUpdate::Plan { .. }));
    assert!(has_tool_update, "should still emit ToolCallUpdated");
    assert!(has_plan, "should also emit Plan");
}

#[test]
fn plan_not_emitted_on_error() {
    let ev = StreamEvent::ToolEnd {
        call_id: Some("call_test".to_string()),
        name: "todo_write".to_string(),
        result: "error".to_string(),
        is_error: true,
        raw_result: None,
    };
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    assert!(plans.is_empty());
}

#[test]
fn plan_not_emitted_on_malformed_result() {
    let ev = make_tool_end("todo_write", "not valid json");
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    assert!(plans.is_empty());
}

#[test]
fn plan_not_emitted_on_empty_todos() {
    let ev = make_tool_end("todo_write", "0 todos\n[]");
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    assert!(plans.is_empty());
}

#[test]
fn plan_works_with_quoted_json_result() {
    let quoted = todo_result_quoted_json();
    let ev = make_tool_end("todo_write", &quoted);
    let updates = loom_event_to_updates(&AnyStreamEvent::React(ev));
    let plans = find_plan_updates(&updates);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].len(), 3);
    assert_eq!(plans[0][0].content, "Analyze the codebase");
}
