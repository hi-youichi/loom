//! End-to-end demo of the crash → cancel → resume flow.
//!
//! Run with:
//!
//!     cargo run -p tool-workflow --example start_cancel_resume
//!
//! Steps it performs:
//!   1. Spawns the `resume_child` subprocess with crash_after=2 — runs the
//!      3-phase script, dispatches a1, then `exit(1)` before a2 starts.
//!   2. Reads the prior `run_dir_name` from the child's stderr.
//!   3. In this process, runs `luft.start_resume(dir)` with a
//!      CountingBackend that records every dispatched agent.
//!   4. Joins to completion and prints the resume outcome.
//!
//! Expected console flow:
//!   - "[child] run_dir: luft-workflow_<ts>"
//!   - "[parent] resuming luft-workflow_<ts>"
//!   - "[parent] dispatched agents: [...]"
//!   - "[parent] outcome: completed"
//!
//! The dispatched agent list should show only `["a2", "a3"]` — a1 is
//! served from the journal cache with zero LLM cost.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use luft::LuftBuilder;
use luft_core::contract::backend::{
    AgentBackend, AgentCapabilities, AgentResult, AgentStatus, AgentTask, BackendError, LogRef,
    RunContext,
};
use luft_core::contract::ids::TokenUsage;
use serde_json::{json, Value};

/// Backend used inside the demo. Records the agent names dispatched in
/// the parent process (post-crash) so we can prove a1 is served from
/// the journal cache.
#[derive(Clone)]
struct CountingBackend {
    canned: Value,
    log: Arc<Mutex<Vec<String>>>,
    seq: Arc<AtomicU64>,
}

impl CountingBackend {
    fn new(canned: Value) -> Self {
        Self {
            canned,
            log: Arc::new(Mutex::new(Vec::new())),
            seq: Arc::new(AtomicU64::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl AgentBackend for CountingBackend {
    fn id(&self) -> &'static str {
        "demo-counting"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            streaming: true,
            mcp_injection: false,
            structured_output: false,
            models: vec![],
        }
    }

    async fn run(
        &self,
        task: AgentTask,
        _ctx: RunContext,
    ) -> Result<AgentResult, BackendError> {
        let _ = self.seq.fetch_add(1, Ordering::SeqCst);
        let name = task.name.clone().unwrap_or_default();
        self.log.lock().unwrap().push(name.clone());
        eprintln!("[parent] dispatched: {name}");
        Ok(AgentResult {
            agent_id: task.agent_id,
            status: AgentStatus::Ok,
            output: self.canned.clone(),
            thread_id: task.thread_id.clone(),
            findings: vec![],
            tokens_used: TokenUsage::default(),
            artifacts: vec![],
            logs: LogRef::default(),
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn child_bin_path() -> std::path::PathBuf {
    let exe = if cfg!(windows) {
        "resume_child.exe"
    } else {
        "resume_child"
    };
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    // Carve out a fresh working folder inside the workspace so the
    // subprocess and the parent don't trample on real instance dirs.
    let candidates = [
        std::path::PathBuf::from(manifest_dir).join("../../target/debug").join(exe),
        std::path::PathBuf::from(manifest_dir).join("../../../target/debug").join(exe),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    std::env::var("CARGO_BIN_EXE_resume_child")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| candidates[0].clone())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = tmp.path().to_path_buf();

    // ── Step 1: spawn subprocess that runs the 3-phase script and exits
    //              after one agent has completed (a1 done, a2 never
    //              starts) — simulating the host crashing mid-workflow.
    let bin = child_bin_path();
    eprintln!("[parent] spawning: {} {} 3phase 2", bin.display(), base.display());
    let output = std::process::Command::new(&bin)
        .arg(&base)
        .arg("3phase")
        .arg("2")
        .output()
        .expect("failed to spawn resume_child");

    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("[parent] child stderr:\n{stderr}");

    assert!(
        !output.status.success(),
        "child should have crashed; got {:?}",
        output.status
    );

    let prior_dir = stderr
        .lines()
        .find_map(|l| l.strip_prefix("[child] run_dir: ").map(String::from))
        .expect("child should have printed run_dir name");
    eprintln!("[parent] prior crashed instance: {prior_dir}");

    // ── Step 2: in the parent, resume the crashed instance.
    let backend = CountingBackend::new(json!("ok"));
    let dispatched = backend.log.clone();
    let luft = LuftBuilder::new()
        .backend(backend)
        .base_dir(&base)
        .concurrency(2)
        .build()
        .expect("luft build");

    eprintln!("[parent] resuming {prior_dir} ...");
    let handle = luft
        .start_resume(&prior_dir)
        .await
        .expect("start_resume");
    let outcome = handle.join().await.expect("join");

    let final_dispatched = dispatched.lock().unwrap().clone();
    let status = if outcome.result.is_ok() {
        "completed"
    } else {
        "errored"
    };
    println!();
    println!("=== resume outcome ===");
    println!("status            : {status}");
    println!("dispatched agents : {:?}", final_dispatched);
    println!("expected          : a2 + a3 (a1 should be journal-cache hit)");
    println!();

    let ok = final_dispatched == vec!["a2".to_string(), "a3".to_string()];
    if ok {
        println!("✅ resume semantics correct: a1 served from cache, a2+a3 re-dispatched.");
        std::process::ExitCode::from(0)
    } else {
        println!("❌ unexpected dispatch sequence.");
        std::process::ExitCode::from(1)
    }
}
