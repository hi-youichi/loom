//! Shared workflow runtime context — path layout, finalization, and
//! checkpoint-driven status inference.
//!
//! All six specialized workflow tools hold an `Arc<WorkflowRuntime>`; this
//! is the single place that knows about the on-disk `.loom` / `.luft`
//! layout, terminal checkpoint detection, instance finalization, and
//! in-memory summary rebuilds.

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tool_core::{ToolCallContent, ToolSourceError};
use tokio_util::sync::CancellationToken;

use crate::common::sanitize_instance_for_public;
use crate::instance::{
    build_instance_meta, write_instance_artifacts, InstanceMeta, ReportRef, WorkflowRef,
};

#[derive(Clone)]
pub struct WorkflowRuntime {
    pub(crate) config_template: agent::agent::AgentConfig,
    pub(crate) active_runs:
        Arc<Mutex<HashMap<String, Arc<CancellationToken>>>>,
}

impl WorkflowRuntime {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            config_template,
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register_run(&self, dir_name: String) -> Arc<CancellationToken> {
        let token = Arc::new(CancellationToken::new());
        self.active_runs
            .lock()
            .expect("active_runs mutex poisoned")
            .insert(dir_name, token.clone());
        token
    }

    pub fn cancel_run(&self, dir_name: &str) -> bool {
        if let Some(token) = self
            .active_runs
            .lock()
            .expect("active_runs mutex poisoned")
            .get(dir_name)
            .cloned()
        {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub fn unregister_run(&self, dir_name: &str) {
        self.active_runs
            .lock()
            .expect("active_runs mutex poisoned")
            .remove(dir_name);
    }

    pub(crate) fn working_folder(&self) -> PathBuf {
        self.config_template
            .working_folder
            .clone()
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub(crate) fn instances_root(&self) -> PathBuf {
        self.working_folder().join(".loom").join("instances")
    }

    pub(crate) fn runs_root(&self) -> PathBuf {
        self.working_folder().join(".luft").join("runs")
    }

    #[allow(dead_code)]
    pub(crate) fn workflows_dir(&self) -> PathBuf {
        self.working_folder().join(".loom").join("workflows")
    }

    pub(crate) fn loom_instance_dir(&self, run_dir_name: &str) -> PathBuf {
        self.instances_root().join(run_dir_name)
    }

    pub(crate) fn resolve_instance_path(&self, instance_dir: &str) -> Option<PathBuf> {
        let new_path = self.instances_root().join(instance_dir);
        if new_path.is_dir() {
            return Some(new_path);
        }
        let legacy_path = self.runs_root().join(instance_dir);
        if legacy_path.is_dir() {
            return Some(legacy_path);
        }
        None
    }

    pub async fn terminal_checkpoint_status(
        &self,
        run_dir_name: &str,
    ) -> Option<&'static str> {
        let path = self.loom_instance_dir(run_dir_name).join("checkpoint.json");
        let bytes = tokio::fs::read(path).await.ok()?;
        let value: Value = serde_json::from_slice(&bytes).ok()?;
        match value.get("status").and_then(Value::as_str) {
            Some("completed") => Some("completed"),
            Some("failed") => Some("failed"),
            Some("cancelled") => Some("cancelled"),
            _ => None,
        }
    }

    pub(crate) async fn finalize(
        &self,
        run_dir_name: &str,
        final_status: &str,
        final_report: Option<&Value>,
        display_name: &str,
        is_inline_script: bool,
        workflow_arg: Option<&str>,
    ) -> Result<(), ToolSourceError> {
        let instance_dir = self.loom_instance_dir(run_dir_name);

        let checkpoint_path = instance_dir.join("checkpoint.json");
        let mut last_err = String::new();
        let mut bytes = None;
        for _ in 0..10 {
            match tokio::fs::read(&checkpoint_path).await {
                Ok(b) if !b.is_empty() => {
                    bytes = Some(b);
                    break;
                }
                Ok(_) => last_err = "workflow state is empty".into(),
                Err(e) => last_err = e.to_string(),
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let checkpoint_bytes = bytes.ok_or_else(|| {
            ToolSourceError::ToolError(format!(
                "workflow state missing or empty after retries: {last_err}"
            ))
        })?;
        let checkpoint: Value = serde_json::from_slice(&checkpoint_bytes)
            .map_err(|e| ToolSourceError::ToolError(format!("invalid workflow state: {e}")))?;

        let events: Vec<Value> = tokio::fs::read_to_string(instance_dir.join("events.jsonl"))
            .await
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        let raw_agent_outputs: Vec<(String, String)> = events
            .iter()
            .filter_map(|ev| {
                if ev.get("type").and_then(|v| v.as_str()) != Some("agent_done") {
                    return None;
                }
                let aid = ev.get("agent_id").and_then(|v| v.as_str())?.to_string();
                let out = ev.get("output")?;
                let raw = match out {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if raw.len() as u64 > crate::instance::AGENT_OUTPUT_INLINE_LIMIT as u64 {
                    Some((aid, raw))
                } else {
                    None
                }
            })
            .collect();

        let workflow_src = std::fs::read_to_string(instance_dir.join("workflow.lua")).ok();

        let workflow_ref = WorkflowRef {
            kind: if is_inline_script { "inline" } else { "file" },
            name: Some(display_name.to_string()),
            path: workflow_arg.map(|s| s.to_string()),
        };

        let mut meta = build_instance_meta(
            &checkpoint,
            &events,
            workflow_src.as_deref(),
            &workflow_ref,
            run_dir_name.to_string(),
            &checkpoint_bytes,
        );

        if final_status != "unknown" {
            meta.status = final_status.to_string();
        }

        write_instance_artifacts(&instance_dir, &meta, final_report, &raw_agent_outputs).map_err(
            |e| ToolSourceError::ToolError(format!("failed to write instance artifacts: {e}")),
        )?;

        Ok(())
    }

    pub(crate) fn write_minimal_failed_instance(
        &self,
        run_dir_name: &str,
        display_name: &str,
        is_inline_script: bool,
        workflow_arg: Option<&str>,
        error_msg: &str,
    ) {
        let instance_dir = self.loom_instance_dir(run_dir_name);
        if std::fs::create_dir_all(&instance_dir).is_err() {
            return;
        }
        let workflow_ref = WorkflowRef {
            kind: if is_inline_script { "inline" } else { "file" },
            name: Some(display_name.to_string()),
            path: workflow_arg.map(|s| s.to_string()),
        };
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let meta = InstanceMeta {
            schema_version: crate::instance::SCHEMA_VERSION,
            instance_id: run_dir_name.to_string(),
            instance_dir: run_dir_name.to_string(),
            workflow: workflow_ref,
            status: "failed".to_string(),
            created_at: now_secs,
            completed_at: now_secs,
            total_tokens: 0,
            total_elapsed_ms: 0,
            agent_count: 0,
            agents: vec![],
            phase_spans: vec![],
            event_stats: Default::default(),
            report: ReportRef::Empty,
            checkpoint_hash: String::new(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&meta) {
            let _ = std::fs::write(instance_dir.join("instance.json"), json);
        }
        eprintln!(
            "warning: workflow {} failed during finalize: {}",
            run_dir_name, error_msg
        );
    }

    pub(crate) async fn rebuild_summary(
        &self,
        instance_dir: &str,
        checkpoint_path: &Path,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let resolved = if checkpoint_path.starts_with(self.instances_root()) {
            self.instances_root().join(instance_dir)
        } else {
            self.runs_root().join(instance_dir)
        };
        let checkpoint_bytes = std::fs::read(checkpoint_path).map_err(|e| {
            ToolSourceError::ToolError(format!("Failed to read workflow state: {e}"))
        })?;
        let checkpoint: Value = serde_json::from_slice(&checkpoint_bytes)
            .map_err(|e| ToolSourceError::ToolError(format!("Invalid workflow state: {e}")))?;
        let events_path = resolved.join("events.jsonl");
        let events: Vec<Value> = match std::fs::read_to_string(&events_path) {
            Ok(raw) => raw
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect(),
            Err(_) => Vec::new(),
        };
        let workflow_src = std::fs::read_to_string(resolved.join("workflow.lua")).ok();
        let workflow_ref = WorkflowRef {
            kind: "legacy",
            name: Some(instance_dir.to_string()),
            path: None,
        };
        let meta = build_instance_meta(
            &checkpoint,
            &events,
            workflow_src.as_deref(),
            &workflow_ref,
            instance_dir.to_string(),
            &checkpoint_bytes,
        );
        let pretty = serde_json::to_string_pretty(&sanitize_instance_for_public(
            serde_json::to_value(&meta).map_err(|e| {
                ToolSourceError::ToolError(format!("Failed to serialise InstanceMeta: {e}"))
            })?,
        ))
        .map_err(|e| ToolSourceError::ToolError(format!("Failed to serialise summary: {e}")))?;
        Ok(ToolCallContent::Text(pretty))
    }
}
