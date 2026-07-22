//! Integration test: `agent({ working_folder = "..." })` propagates the
//! override through the full pipeline (Lua → build_task → AgentTask → backend).

use luft::LuftBuilder;
use luft_core::contract::backend::{
    AgentBackend, AgentCapabilities, AgentResult, AgentStatus, AgentTask, BackendError, LogRef,
    RunContext,
};
use luft_core::contract::ids::TokenUsage;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
// Note: CapturingBackend is passed by value to LuftBuilder; Arc<Mutex<_>> is
// used internally to share the capture vec across the moved backend.

/// Captures the `workdir_override` seen by the backend.
struct CapturingBackend {
    seen: Arc<Mutex<Vec<Option<PathBuf>>>>,
}

#[async_trait::async_trait]
impl AgentBackend for CapturingBackend {
    fn id(&self) -> &'static str {
        "capture"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            streaming: true,
            mcp_injection: false,
            structured_output: false,
            models: vec![],
        }
    }

    async fn run(&self, task: AgentTask, _ctx: RunContext) -> Result<AgentResult, BackendError> {
        self.seen.lock().unwrap().push(task.workdir_override.clone());
        Ok(AgentResult {
            agent_id: task.agent_id,
            status: AgentStatus::Ok,
            output: Value::String("done".into()),
            findings: vec![],
            tokens_used: TokenUsage::default(),
            artifacts: vec![],
            logs: LogRef::default(),
            thread_id: task.thread_id.clone(),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[tokio::test]
async fn working_folder_propagates_to_backend() {
    let seen: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::new(Mutex::new(vec![]));

    let backend = CapturingBackend {
        seen: seen.clone(),
    };

    let tmp = tempfile::TempDir::new().unwrap();

    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(tmp.path())
        .concurrency(2)
        .build()
        .unwrap();

    let script = r#"
        function main()
          phase("test")
          local r = agent({
            name = "with-folder",
            prompt = "do something",
            working_folder = "/custom/path/from/lua",
          })
          if not r.ok then
            report({ error = "agent failed" })
            return
          end
          report({ result = r.output })
        end
    "#;

    let handle = luft.start_script(script).await.unwrap();
    handle.join().await.unwrap();

    let captured = seen.lock().unwrap();
    assert_eq!(captured.len(), 1, "exactly one agent should have run");
    assert_eq!(
        captured[0],
        Some(PathBuf::from("/custom/path/from/lua")),
        "workdir_override must match the Lua working_folder value",
    );
}

#[tokio::test]
async fn no_working_folder_defaults_to_none() {
    let seen: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::new(Mutex::new(vec![]));

    let backend = CapturingBackend {
        seen: seen.clone(),
    };

    let tmp = tempfile::TempDir::new().unwrap();

    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(tmp.path())
        .concurrency(2)
        .build()
        .unwrap();

    let script = r#"
        function main()
          phase("test")
          local r = agent({
            prompt = "no folder override",
          })
          report({ result = r.output })
        end
    "#;

    let handle = luft.start_script(script).await.unwrap();
    handle.join().await.unwrap();

    let captured = seen.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0], None,
        "without working_folder, workdir_override must be None",
    );
}

#[tokio::test]
async fn parallel_propagates_working_folder() {
    let seen: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::new(Mutex::new(vec![]));

    let backend = CapturingBackend {
        seen: seen.clone(),
    };

    let tmp = tempfile::TempDir::new().unwrap();

    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(tmp.path())
        .concurrency(4)
        .build()
        .unwrap();

    let script = r#"
        function main()
          phase("test")
          local items = { "alpha", "beta" }
          local results = parallel(items, function(item)
            return {
              prompt = "process " .. item,
              working_folder = "/work/" .. item,
            }
          end)
          report({ count = #results })
        end
    "#;

    let handle = luft.start_script(script).await.unwrap();
    handle.join().await.unwrap();

    let captured = seen.lock().unwrap();
    assert_eq!(captured.len(), 2, "two parallel agents should have run");
    let paths: Vec<String> = captured
        .iter()
        .map(|p| p.as_ref().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        paths.iter().any(|p| p == "/work/alpha"),
        "expected /work/alpha in {:?}",
        paths,
    );
    assert!(
        paths.iter().any(|p| p == "/work/beta"),
        "expected /work/beta in {:?}",
        paths,
    );
}
