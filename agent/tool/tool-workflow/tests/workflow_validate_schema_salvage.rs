//! Integration test: workflow_validate_schema salvage.
//!
//! When a workflow agent captures `workflow_validate_schema` but then crashes on a
//! follow-up LLM call, the backend should salvage the captured output rather
//! than reporting a hard failure. This test verifies the contract at the
//! workflow level using a mock backend that simulates the scenario.

use luft::LuftBuilder;
use luft_core::contract::backend::{
    AgentBackend, AgentCapabilities, AgentResult, AgentStatus, AgentTask, BackendError, LogRef,
    RunContext,
};
use luft_core::contract::ids::TokenUsage;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// Backend that simulates the salvage scenario:
/// - First N calls succeed with structured output
/// - Call N+1 fails (simulating post-workflow_validate_schema LLM crash)
struct SalvageBackend {
    outputs: Arc<Mutex<Vec<Value>>>,
}

#[async_trait::async_trait]
impl AgentBackend for SalvageBackend {
    fn id(&self) -> &'static str {
        "salvage"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            streaming: false,
            mcp_injection: false,
            workflow_validate_schema: true,
            session_resume: false,
            models: vec![],
        }
    }

    async fn run(&self, task: AgentTask, _ctx: RunContext) -> Result<AgentResult, BackendError> {
        let outputs = self.outputs.lock().unwrap();
        let agent_id = task.agent_id;

        // Simulate: workflow_validate_schema was captured, then agent.run() failed.
        // Backend salvages: returns Ok with captured structured output.
        let output = outputs
            .iter()
            .find(|v| v.get("agent_id").and_then(|a| a.as_str()) == Some(&agent_id.to_string()))
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "changed": true,
                    "files": ["foundation/stream-event/src/types/stream_event.rs"],
                    "summary": "Phase A complete: refactored StreamEvent enum",
                })
            });

        Ok(AgentResult {
            agent_id: task.agent_id,
            status: AgentStatus::Ok,
            output,
            findings: vec![],
            tokens_used: TokenUsage::default(),
            artifacts: vec![],
            logs: LogRef::default(),
            session_id: task.session_id.clone(),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Backend that always fails — simulates the pre-fix behavior where
/// structured_output is captured but then lost.
struct FailingBackend;

#[async_trait::async_trait]
impl AgentBackend for FailingBackend {
    fn id(&self) -> &'static str {
        "fail"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities::default()
    }

    async fn run(&self, task: AgentTask, _ctx: RunContext) -> Result<AgentResult, BackendError> {
        Err(BackendError::Execution(format!(
            "agent {} failed: LLM timed out after workflow_validate_schema",
            task.agent_id
        )))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[tokio::test]
async fn salvage_backend_produces_structured_output_in_workflow() {
    let backend = SalvageBackend {
        outputs: Arc::new(Mutex::new(vec![])),
    };

    let tmp = tempfile::TempDir::new().unwrap();

    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(tmp.path())
        .concurrency(1)
        .build()
        .unwrap();

    let script = r#"
        function main()
          phase("A")
          local a = agent({
            name = "A-enum",
            prompt = "Refactor StreamEvent enum",
            schema = {
              type = "object",
              properties = {
                changed = { type = "boolean" },
                files = { type = "array", items = { type = "string" } },
                summary = { type = "string" },
              },
              required = { "changed", "summary" },
            },
          })
          if not a.ok then
            report({ error = "Phase A failed: " .. a.status })
            return
          end
          report({ result = a.output })
        end
    "#;

    let handle = luft.start_script(script).await.unwrap();
    let result = handle.join().await;

    let outcome = result.unwrap();
    let report = outcome
        .result
        .expect("workflow should succeed with salvage");
    let result_val = report.get("result").expect("report should contain result");
    assert_eq!(
        result_val.get("changed").and_then(|v| v.as_bool()),
        Some(true),
        "salvaged workflow_validate_schema should contain changed=true"
    );
}

#[tokio::test]
async fn failing_backend_causes_workflow_failure() {
    let backend = FailingBackend;

    let tmp = tempfile::TempDir::new().unwrap();

    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(tmp.path())
        .concurrency(1)
        .build()
        .unwrap();

    let script = r#"
        function main()
          phase("A")
          local a = agent({
            name = "A-fail",
            prompt = "This will fail",
          })
          if not a.ok then
            report({ error = "Phase A failed as expected" })
            return
          end
          report({ result = a.output })
        end
    "#;

    let handle = luft.start_script(script).await.unwrap();
    let result = handle.join().await;

    let outcome = result.unwrap();
    assert!(
        outcome.result.is_err(),
        "workflow should fail when backend has no workflow_validate_schema to salvage"
    );
}
