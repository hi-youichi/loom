//! Evolution run persistence and version management.

use crate::types::EvolutionResult;
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

/// Manages evolution run records on the filesystem.
pub struct RunStore {
    base_dir: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunSummary {
    pub skill_name: String,
    pub timestamp: String,
    pub baseline_score: f64,
    pub evolved_score: f64,
    pub accepted: bool,
}

impl RunStore {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Save an evolution run result.
    pub fn save_run(&self, result: &EvolutionResult) -> Result<PathBuf, DeployError> {
        let run_dir = self.base_dir
            .join("runs")
            .join(&result.skill_name)
            .join(result.timestamp.format("%Y%m%d_%H%M%S").to_string());

        fs::create_dir_all(&run_dir)?;

        // Save metrics
        let metrics_path = run_dir.join("metrics.json");
        let metrics_json = serde_json::to_string_pretty(result)?;
        fs::write(&metrics_path, metrics_json)?;

        // Save evolved content
        let evolved_path = run_dir.join("evolved.md");
        fs::write(&evolved_path, &result.evolved_content)?;

        // Save a human-readable diff summary
        let summary = format!(
            "# Evolution Run: {}\n\n\
             - Date: {}\n\
             - Baseline score: {:.3}\n\
             - Evolved score: {:.3}\n\
             - Improvement: {:.1}%\n\
             - Size: {} -> {} bytes ({:.2}x)\n\
             - Iterations: {}\n\
             - Candidates evaluated: {}\n\
             - Constraints passed: {}\n\
             - Constraints failed: {}\n",
            result.skill_name,
            result.timestamp,
            result.baseline_score,
            result.evolved_score,
            (result.evolved_score - result.baseline_score) / result.baseline_score.max(0.001) * 100.0,
            result.baseline_size,
            result.evolved_size,
            result.size_ratio,
            result.iterations,
            result.candidates_evaluated,
            result.constraints_passed.join(", "),
            result.constraints_failed.join(", "),
        );
        fs::write(run_dir.join("diff.md"), summary)?;

        Ok(run_dir)
    }

    /// Save baseline backup before accepting an evolution.
    pub fn save_baseline_backup(
        &self,
        skill_name: &str,
        baseline_content: &str,
    ) -> Result<PathBuf, DeployError> {
        let backup_dir = self.base_dir.join("backups").join(skill_name);
        fs::create_dir_all(&backup_dir)?;

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let backup_path = backup_dir.join(format!("{}.md", timestamp));
        fs::write(&backup_path, baseline_content)?;

        Ok(backup_path)
    }

    /// List available backups for a skill.
    pub fn list_backups(&self, skill_name: &str) -> Result<Vec<String>, DeployError> {
        let backup_dir = self.base_dir.join("backups").join(skill_name);
        if !backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups: Vec<String> = fs::read_dir(&backup_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "md").unwrap_or(false))
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            })
            .collect();

        backups.sort();
        backups.reverse(); // Most recent first
        Ok(backups)
    }

    pub fn default_path() -> PathBuf {
        let home = std::env::var("LOOM_HOME")
            .ok()
            .or_else(|| std::env::var("HOME").ok().map(|h| format!("{}/.loom", h)))
            .unwrap_or_else(|| "~/.loom".to_string());
        PathBuf::from(home).join("data").join("evolution")
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<RunSummary>, DeployError> {
        let runs_dir = self.base_dir.join("runs");
        if !runs_dir.exists() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();
        for skill_entry in fs::read_dir(&runs_dir)?.filter_map(|e| e.ok()) {
            if !skill_entry.file_type()?.is_dir() {
                continue;
            }
            for run_entry in fs::read_dir(skill_entry.path())?.filter_map(|e| e.ok()) {
                let metrics_path = run_entry.path().join("metrics.json");
                if let Ok(data) = fs::read_to_string(&metrics_path) {
                    if let Ok(result) = serde_json::from_str::<EvolutionResult>(&data) {
                        summaries.push(RunSummary {
                            skill_name: result.skill_name.clone(),
                            timestamp: result.timestamp.to_rfc3339(),
                            baseline_score: result.baseline_score,
                            evolved_score: result.evolved_score,
                            accepted: result.accepted,
                        });
                    }
                }
            }
        }

        summaries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        summaries.truncate(limit);
        Ok(summaries)
    }

    pub fn load_latest(&self, skill_name: &str) -> Result<Option<EvolutionResult>, DeployError> {
        let runs_dir = self.base_dir.join("runs").join(skill_name);
        if !runs_dir.exists() {
            return Ok(None);
        }

        let mut entries: Vec<String> = fs::read_dir(&runs_dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .collect();
        entries.sort();
        entries.reverse();

        for name in &entries {
            let metrics_path = runs_dir.join(name).join("metrics.json");
            if let Ok(data) = fs::read_to_string(&metrics_path) {
                if let Ok(result) = serde_json::from_str::<EvolutionResult>(&data) {
                    return Ok(Some(result));
                }
            }
        }
        Ok(None)
    }

    pub fn accept(&self, skill_name: &str) -> Result<(), DeployError> {
        if let Some(mut result) = self.load_latest(skill_name)? {
            result.accepted = true;
            let run_dir = self.base_dir
                .join("runs")
                .join(skill_name)
                .join(result.timestamp.format("%Y%m%d_%H%M%S").to_string());
            if run_dir.exists() {
                let metrics_path = run_dir.join("metrics.json");
                let data = serde_json::to_string_pretty(&result)?;
                fs::write(&metrics_path, data)?;
            }
        }
        Ok(())
    }

    pub fn reject(&self, skill_name: &str) -> Result<(), DeployError> {
        if let Some(mut result) = self.load_latest(skill_name)? {
            result.accepted = false;
            let run_dir = self.base_dir
                .join("runs")
                .join(skill_name)
                .join(result.timestamp.format("%Y%m%d_%H%M%S").to_string());
            if run_dir.exists() {
                let metrics_path = run_dir.join("metrics.json");
                let data = serde_json::to_string_pretty(&result)?;
                fs::write(&metrics_path, data)?;
            }
        }
        Ok(())
    }

    /// Load a backup by version string (timestamp).
    pub fn load_backup(
        &self,
        skill_name: &str,
        version: &str,
    ) -> Result<String, DeployError> {
        let path = self.base_dir
            .join("backups")
            .join(skill_name)
            .join(format!("{}.md", version));

        if !path.exists() {
            return Err(DeployError::NotFound(format!(
                "Backup not found: {} version {}",
                skill_name, version
            )));
        }

        Ok(fs::read_to_string(&path)?)
    }

    /// Rollback a skill to a previous version.
    pub fn rollback(
        &self,
        skill_name: &str,
        version: &str,
        current_skill_path: &Path,
    ) -> Result<(), DeployError> {
        let content = self.load_backup(skill_name, version)?;
        fs::write(current_skill_path, &content)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use chrono::Utc;

    fn make_result() -> EvolutionResult {
        EvolutionResult {
            skill_name: "test-skill".to_string(),
            timestamp: Utc::now(),
            optimizer: "GEPA".to_string(),
            iterations: 5,
            candidates_evaluated: 25,
            baseline_score: 0.60,
            evolved_score: 0.78,
            holdout_score: Some(0.75),
            baseline_size: 1000,
            evolved_size: 1150,
            size_ratio: 1.15,
            dataset_source: "synthetic".to_string(),
            dataset_size: 20,
            cost_usd: Some(3.50),
            constraints_passed: vec!["size_budget".to_string(), "structure_integrity".to_string()],
            constraints_failed: vec![],
            regression_check: Some("passed".to_string()),
            accepted: false,
            evolved_content: "---\nname: test-skill\ndescription: evolved\n---\nEvolved content.".to_string(),
        }
    }

    #[test]
    fn save_and_load_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = RunStore::new(dir.path());
        let result = make_result();

        let run_dir = store.save_run(&result).unwrap();
        assert!(run_dir.join("metrics.json").exists());
        assert!(run_dir.join("evolved.md").exists());
        assert!(run_dir.join("diff.md").exists());

        let metrics: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(run_dir.join("metrics.json")).unwrap()).unwrap();
        assert_eq!(metrics["skill_name"], "test-skill");
        assert_eq!(metrics["evolved_score"], 0.78);
    }

    #[test]
    fn backup_and_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let store = RunStore::new(dir.path());

        let baseline = "---\nname: test\ndescription: original\n---\nOriginal.";
        store.save_baseline_backup("test-skill", baseline).unwrap();

        let backups = store.list_backups("test-skill").unwrap();
        assert_eq!(backups.len(), 1);

        let loaded = store.load_backup("test-skill", &backups[0]).unwrap();
        assert_eq!(loaded, baseline);

        // Rollback
        let skill_file = dir.path().join("current_skill.md");
        std::fs::write(&skill_file, "wrong content").unwrap();
        store.rollback("test-skill", &backups[0], &skill_file).unwrap();
        assert_eq!(std::fs::read_to_string(&skill_file).unwrap(), baseline);
    }

    #[test]
    fn list_backups_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = RunStore::new(dir.path());
        let backups = store.list_backups("nonexistent").unwrap();
        assert!(backups.is_empty());
    }
}
