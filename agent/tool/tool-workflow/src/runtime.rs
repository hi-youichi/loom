//! Shared workflow runtime context — path layout, finalization, and
//! checkpoint-driven status inference.
//!
//! All six specialized workflow tools hold an `Arc<WorkflowRuntime>`; this
//! is the single place that knows about the on-disk `.loom` / `.luft`
//! layout, terminal checkpoint detection, instance finalization, and
//! in-memory summary rebuilds.

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use tool_core::{ToolCallContent, ToolSourceError};

use crate::common::sanitize_instance_for_public;
use crate::instance::{
    build_instance_meta, write_instance_artifacts, InstanceMeta, ReportRef, WorkflowRef,
};

/// Map a `CheckpointStatus` to a terminal status string (`"completed"`,
/// `"failed"`, `"cancelled"`), or `None` if still running.
fn checkpoint_terminal_str(status: &luft_core::state::CheckpointStatus) -> Option<&'static str> {
    use luft_core::state::CheckpointStatus;
    match status {
        CheckpointStatus::Completed => Some("completed"),
        CheckpointStatus::Failed => Some("failed"),
        CheckpointStatus::Cancelled => Some("cancelled"),
        CheckpointStatus::Running => None,
    }
}

/// Convert a luft checkpoint + events pair into the JSON values expected by
/// `build_instance_meta`. Returns `(checkpoint_value, checkpoint_bytes, events_values)`.
fn checkpoint_to_json(
    checkpoint: &luft_core::state::RunCheckpoint,
    events: &[luft_core::contract::event::AgentEvent],
) -> (Value, Vec<u8>, Vec<Value>) {
    let checkpoint_bytes = serde_json::to_vec(checkpoint).unwrap_or_default();
    let checkpoint_value = serde_json::to_value(checkpoint).unwrap_or(Value::Null);
    let events_values: Vec<Value> = events
        .iter()
        .filter_map(|e| serde_json::to_value(e).ok())
        .collect();
    (checkpoint_value, checkpoint_bytes, events_values)
}

#[derive(Clone)]
pub struct WorkflowRuntime {
    pub(crate) config_template: agent::agent::AgentConfig,
    pub(crate) active_runs: Arc<Mutex<HashMap<String, Arc<CancellationToken>>>>,
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
            .insert(dir_name.clone(), token.clone());
        tracing::debug!(
            target: "workflow::runtime",
            instance_dir = %dir_name,
            "registered run in active_runs registry",
        );
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
            tracing::debug!(
                target: "workflow::runtime",
                instance_dir = %dir_name,
                "cancel_run: token fired",
            );
            true
        } else {
            tracing::debug!(
                target: "workflow::runtime",
                instance_dir = %dir_name,
                "cancel_run: not found in registry",
            );
            false
        }
    }

    pub fn unregister_run(&self, dir_name: &str) {
        self.active_runs
            .lock()
            .expect("active_runs mutex poisoned")
            .remove(dir_name);
        tracing::debug!(
            target: "workflow::runtime",
            instance_dir = %dir_name,
            "unregistered run from active_runs registry",
        );
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

    pub async fn terminal_checkpoint_status(&self, run_dir_name: &str) -> Option<&'static str> {
        let base_dir = self.instances_root();
        let owned = run_dir_name.to_string();
        let cp =
            tokio::task::spawn_blocking(move || luft::query::get_checkpoint(&owned, &base_dir))
                .await
                .ok()?
                .ok()??;
        checkpoint_terminal_str(&cp.status)
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
        let base_dir = self.instances_root();

        tracing::debug!(
            target: "workflow::runtime",
            instance_dir = %instance_dir.display(),
            final_status,
            "finalize: querying SQLite checkpoint",
        );

        let owned_run_dir = run_dir_name.to_string();
        let owned_base_dir = base_dir.clone();
        let mut last_err = String::new();
        let mut checkpoint_opt = None;
        for attempt in 0..10 {
            let rd = owned_run_dir.clone();
            let bd = owned_base_dir.clone();
            let result =
                tokio::task::spawn_blocking(move || luft::query::get_checkpoint(&rd, &bd)).await;
            match result {
                Ok(Ok(Some(cp))) => {
                    checkpoint_opt = Some(cp);
                    break;
                }
                Ok(Ok(None)) => last_err = "checkpoint not found in SQLite".into(),
                Ok(Err(e)) => last_err = e.to_string(),
                Err(e) => last_err = format!("join error: {e}"),
            }
            if attempt == 0 {
                tracing::warn!(
                    target: "workflow::runtime",
                    instance_dir = %instance_dir.display(),
                    "finalize: checkpoint not ready, retrying (up to 10x 200ms)",
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let checkpoint = checkpoint_opt.ok_or_else(|| {
            ToolSourceError::ToolError(format!(
                "workflow state missing or empty after retries: {last_err}"
            ))
        })?;

        let owned_run_dir2 = run_dir_name.to_string();
        let owned_base_dir2 = base_dir.clone();
        let events = tokio::task::spawn_blocking(move || {
            luft::query::get_events(&owned_run_dir2, &owned_base_dir2)
        })
        .await
        .map_err(|e| ToolSourceError::ToolError(format!("events query join error: {e}")))?
        .map_err(|e| ToolSourceError::ToolError(format!("Failed to read events: {e}")))?;

        let (checkpoint_value, checkpoint_bytes, events_values) =
            checkpoint_to_json(&checkpoint, &events);

        if let Ok(json) = serde_json::to_string_pretty(&checkpoint) {
            let _ = std::fs::write(instance_dir.join("checkpoint.json"), json);
        }

        let raw_agent_outputs: Vec<(String, String)> = events_values
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
            &checkpoint_value,
            &events_values,
            workflow_src.as_deref(),
            &workflow_ref,
            run_dir_name.to_string(),
            &checkpoint_bytes,
        );

        if final_status != "unknown" {
            meta.status = final_status.to_string();
        }

        write_instance_artifacts(&instance_dir, &meta, final_report, &raw_agent_outputs).map_err(
            |e| {
                tracing::error!(
                    target: "workflow::runtime",
                    instance_dir = %instance_dir.display(),
                    error = %e,
                    "finalize: failed to write instance artifacts",
                );
                ToolSourceError::ToolError(format!("failed to write instance artifacts: {e}"))
            },
        )?;

        tracing::info!(
            target: "workflow::runtime",
            instance_dir = %instance_dir.display(),
            status = %meta.status,
            agent_count = meta.agents.len(),
            "finalize: instance artifacts written successfully",
        );

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
        tracing::error!(
            target: "workflow::runtime",
            instance_dir = run_dir_name,
            error = error_msg,
            "writing minimal failed instance",
        );
        let instance_dir = self.loom_instance_dir(run_dir_name);
        if std::fs::create_dir_all(&instance_dir).is_err() {
            tracing::error!(
                target: "workflow::runtime",
                instance_dir = %instance_dir.display(),
                "write_minimal_failed_instance: could not create directory",
            );
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
    }

    #[allow(dead_code)]
    pub(crate) async fn rebuild_summary(
        &self,
        instance_dir: &str,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let base_dir = self.instances_root();
        let owned_dir = instance_dir.to_string();
        let owned_base = base_dir.clone();
        let checkpoint = tokio::task::spawn_blocking(move || {
            luft::query::get_checkpoint(&owned_dir, &owned_base)
        })
        .await
        .map_err(|e| ToolSourceError::ToolError(format!("checkpoint query join error: {e}")))?
        .map_err(|e| ToolSourceError::ToolError(format!("Failed to read workflow state: {e}")))?
        .ok_or_else(|| ToolSourceError::ToolError("workflow state not found".to_string()))?;

        let owned_dir2 = instance_dir.to_string();
        let owned_base2 = base_dir.clone();
        let events =
            tokio::task::spawn_blocking(move || luft::query::get_events(&owned_dir2, &owned_base2))
                .await
                .map_err(|e| ToolSourceError::ToolError(format!("events query join error: {e}")))?
                .unwrap_or_default();

        let resolved = self.loom_instance_dir(instance_dir);
        let workflow_src = std::fs::read_to_string(resolved.join("workflow.lua")).ok();
        let workflow_ref = WorkflowRef {
            kind: "legacy",
            name: Some(instance_dir.to_string()),
            path: None,
        };

        let (checkpoint_value, checkpoint_bytes, events_values) =
            checkpoint_to_json(&checkpoint, &events);
        let meta = build_instance_meta(
            &checkpoint_value,
            &events_values,
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
