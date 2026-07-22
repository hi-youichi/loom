//! Regression test: prove that `parallel(items, mapper)` with a mapper that
//! returns tables without a `prompt` key surfaces as a `ToolError` —
//! and that failed runs propagate as errors rather than silently returning
//! `Ok(Text)`. The actual Lua error ("agent: missing required 'prompt'
//! field") is extracted via `RunHandle::join()` and prefixed with
//! "Workflow failed:" so the agent loop receives a human-readable message.
//!
//! Reproduction path:
//! 1. User writes `parallel({"a", "b"}, function(item) return {topic = item} end)`
//! 2. luft calls the mapper → gets `{"topic": "a"}` (no `prompt`)
//! 3. `build_task()` in luft-runtime/sdk/task.rs rejects it with the Lua
//!    RuntimeError "agent: missing required 'prompt' field"
//! 4. luft marks the run as `Failed`; `RunDone` event has `report: Null`
//! 5. tool.rs calls `run_handle.join()` to get the actual `ScriptError`
//! 6. The error is surfaced as `ToolError("Workflow failed: ...")`

use tool_core::Tool;
use tool_workflow::WorkflowTool;

fn make_tool() -> WorkflowTool {
    use agent::agent::AgentConfig;
    WorkflowTool::new(AgentConfig::default())
}

/// Verify that failed runs propagate as ToolError with the actual Lua error.
/// The error must mention "Workflow failed" (human-readable prefix) AND
/// contain the underlying cause ("prompt" or "required").
#[tokio::test]
async fn parallel_mapper_missing_prompt_fails_with_tool_error() {
    let tool = make_tool();

    let script = r#"
        function main()
            local results = parallel({"a", "b"}, function(item)
                return {topic = item}
            end)
            report({results = results})
        end
    "#;

    let args = serde_json::json!({
        "action": "run",
        "script": script,
    });

    let result = tool.call(args, None).await;

    let err = result.expect_err("workflow must fail when mapper omits prompt");
    let err_str = err.to_string();

    assert!(
        err_str.contains("Workflow failed"),
        "error should say 'Workflow failed', got: {err_str}",
    );
    assert!(
        err_str.contains("prompt"),
        "error should mention 'prompt' (the underlying Lua error), got: {err_str}",
    );
}

/// Verify that parallel with a direct table-of-tables without `prompt`
/// also fails correctly. When no mapper is supplied, luft requires
/// a `Function` argument — the error surfaces as "Workflow failed".
#[tokio::test]
async fn parallel_direct_table_without_prompt_fails() {
    let tool = make_tool();

    let script = r#"
        function main()
            local results = parallel({{topic = "x"}, {topic = "y"}})
            report({results = results})
        end
    "#;

    let args = serde_json::json!({
        "action": "run",
        "script": script,
    });

    let result = tool.call(args, None).await;

    let err = result.expect_err("parallel with table missing prompt must fail");
    let err_str = err.to_string();
    assert!(
        err_str.contains("Workflow failed"),
        "error should say 'Workflow failed', got: {err_str}",
    );
}

/// Verify that agent() itself without prompt also propagates as error
/// with the actual Lua error visible.
#[tokio::test]
async fn agent_without_prompt_fails() {
    let tool = make_tool();

    let script = r#"
        function main()
            agent({topic = "no prompt"})
            report({ok = true})
        end
    "#;

    let args = serde_json::json!({
        "action": "run",
        "script": script,
    });

    let result = tool.call(args, None).await;

    let err = result.expect_err("agent without prompt must fail");
    let err_str = err.to_string();
    assert!(
        err_str.contains("Workflow failed"),
        "error should say 'Workflow failed', got: {err_str}",
    );
    assert!(
        err_str.contains("prompt"),
        "error should mention 'prompt' (the underlying Lua error), got: {err_str}",
    );
}

/// Verify that successful runs still return Ok with content.
#[tokio::test]
async fn agent_with_prompt_succeeds() {
    let tool = make_tool();

    let script = r#"
        function main()
            agent({prompt = "Return JSON: {\"ok\": true}"})
            report({ok = true})
        end
    "#;

    let args = serde_json::json!({
        "action": "run",
        "script": script,
    });

    let result = tool.call(args, None).await;
    assert!(
        result.is_ok(),
        "agent with valid prompt should succeed, got: {:?}",
        result,
    );
}

/// Verify that a Lua syntax error also surfaces as a Workflow failure
/// with the actual syntax error text visible.
#[tokio::test]
async fn lua_syntax_error_surfaces() {
    let tool = make_tool();

    let script = "function main() this is not valid lua end";

    let args = serde_json::json!({
        "action": "run",
        "script": script,
    });

    let result = tool.call(args, None).await;

    let err = result.expect_err("syntax errors must surface as ToolError");
    let err_str = err.to_string();
    assert!(
        err_str.contains("Workflow"),
        "syntax error should mention 'Workflow', got: {err_str}",
    );
}

/// Verify that a report with a non-null value returns Ok with the
/// report content (the happy path for successful workflows).
#[tokio::test]
async fn report_value_returns_ok() {
    let tool = make_tool();

    let script = r#"
        function main()
            report({status = "done", count = 42})
        end
    "#;

    let args = serde_json::json!({
        "action": "run",
        "script": script,
    });

    let result = tool.call(args, None).await;
    assert!(result.is_ok(), "simple report should succeed");
    let text = result.unwrap().to_string();
    assert!(text.contains("done"), "report should contain status text");
    assert!(text.contains("42"), "report should contain count");
}
