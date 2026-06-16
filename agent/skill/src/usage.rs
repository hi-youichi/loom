//! Skill usage tracking — telemetry and lifecycle management.
//!
//! This module provides `SkillUsageStore` for tracking skill usage statistics
//! and lifecycle state.

use crate::storage::Lifecycle;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Skill usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUsage {
    pub name: String,
    #[serde(default)]
    pub use_count: u64,
    #[serde(default)]
    pub view_count: u64,
    #[serde(default)]
    pub patch_count: u64,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub last_viewed_at: Option<String>,
    #[serde(default)]
    pub last_patched_at: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default = "default_state")]
    pub state: Lifecycle,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub absorbed_into: Option<String>,
}

fn default_state() -> Lifecycle {
    Lifecycle::Active
}

impl SkillUsage {
    /// Create a new usage record for a skill.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            use_count: 0,
            view_count: 0,
            patch_count: 0,
            last_used_at: None,
            last_viewed_at: None,
            last_patched_at: None,
            created_at: Utc::now().to_rfc3339(),
            created_by: None,
            state: Lifecycle::Active,
            pinned: false,
            archived_at: None,
            absorbed_into: None,
        }
    }

    /// Get the timestamp of the last activity (use, view, or patch).
    pub fn last_activity_at(&self) -> Option<String> {
        let candidates = [
            self.last_used_at.as_deref(),
            self.last_viewed_at.as_deref(),
            self.last_patched_at.as_deref(),
        ];
        candidates
            .iter()
            .filter_map(|&s| s)
            .max()
            .map(|s| s.to_string())
    }

    /// Get total activity count.
    pub fn activity_count(&self) -> u64 {
        self.use_count + self.view_count + self.patch_count
    }
}

/// A usage report for agent-created skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUsageReport {
    pub name: String,
    pub use_count: u64,
    pub view_count: u64,
    pub patch_count: u64,
    pub last_used_at: Option<String>,
    pub last_viewed_at: Option<String>,
    pub last_patched_at: Option<String>,
    pub created_at: String,
    pub state: Lifecycle,
    pub pinned: bool,
    pub archived_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub activity_count: u64,
}

impl From<(String, SkillUsage)> for SkillUsageReport {
    fn from((name, usage): (String, SkillUsage)) -> Self {
        let last_activity_at = usage.last_activity_at();
        let activity_count = usage.activity_count();
        Self {
            name,
            use_count: usage.use_count,
            view_count: usage.view_count,
            patch_count: usage.patch_count,
            last_used_at: usage.last_used_at,
            last_viewed_at: usage.last_viewed_at,
            last_patched_at: usage.last_patched_at,
            created_at: usage.created_at,
            state: usage.state,
            pinned: usage.pinned,
            archived_at: usage.archived_at,
            last_activity_at,
            activity_count,
        }
    }
}

/// Store for skill usage statistics.
pub struct SkillUsageStore {
    path: PathBuf,
}

impl SkillUsageStore {
    /// Create a new usage store at the given base directory.
    pub fn new(base_dir: &Path) -> Self {
        Self {
            path: base_dir.join(".usage.json"),
        }
    }

    /// Increment the use count for a skill.
    pub fn bump_use(&self, name: &str) {
        self.update(name, |u| {
            u.use_count += 1;
            u.last_used_at = Some(Utc::now().to_rfc3339());
        });
    }

    /// Increment the view count for a skill.
    pub fn bump_view(&self, name: &str) {
        self.update(name, |u| {
            u.view_count += 1;
            u.last_viewed_at = Some(Utc::now().to_rfc3339());
        });
    }

    /// Increment the patch count for a skill.
    pub fn bump_patch(&self, name: &str) {
        self.update(name, |u| {
            u.patch_count += 1;
            u.last_patched_at = Some(Utc::now().to_rfc3339());
        });
    }

    /// Mark a skill as agent-created.
    pub fn mark_agent_created(&self, name: &str) {
        self.update(name, |u| {
            u.created_by = Some("agent".to_string());
        });
    }

    /// Check if a skill was created by the agent (background review).
    pub fn is_agent_created(&self, name: &str) -> bool {
        self.get(name)
            .and_then(|u| u.created_by)
            .map(|by| by == "agent")
            .unwrap_or(false)
    }

    /// List all skill names created by the agent.
    pub fn agent_created_names(&self) -> Vec<String> {
        self.load()
            .map(|data| {
                data.iter()
                    .filter(|(_, u)| u.created_by.as_deref() == Some("agent"))
                    .map(|(name, _)| name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Update the lifecycle state of a skill.
    pub fn set_state(&self, name: &str, state: Lifecycle) {
        self.update(name, |u| {
            u.state = state;
            if matches!(state, Lifecycle::Archived) {
                u.archived_at = Some(Utc::now().to_rfc3339());
            }
        });
    }

    /// Set the pinned status of a skill.
    pub fn set_pinned(&self, name: &str, pinned: bool) {
        self.update(name, |u| {
            u.pinned = pinned;
        });
    }

    /// Remove usage data for a skill, optionally recording the deletion intent.
    pub fn forget(&self, name: &str) {
        self.forget_with_intent(name, None);
    }

    /// Remove usage data for a skill, recording where it was absorbed into.
    pub fn forget_with_intent(&self, name: &str, absorbed_into: Option<&str>) {
        match self.load() {
            Ok(mut data) => {
                if let Some(target) = absorbed_into {
                    if let Some(entry) = data.get_mut(name) {
                        entry.absorbed_into = Some(target.to_string());
                    }
                }
                data.remove(name);
                if let Err(e) = self.save(&data) {
                    debug!("SkillUsageStore: forget '{}' failed: {}", name, e);
                }
            }
            Err(e) => {
                debug!("SkillUsageStore: forget '{}' failed to load: {}", name, e);
            }
        }
    }

    /// Get a report of all agent-created skills.
    pub fn agent_created_report(&self) -> Result<Vec<SkillUsageReport>, String> {
        let data = self.load().map_err(|e| e.to_string())?;
        let rows: Vec<SkillUsageReport> = data
            .into_iter()
            .filter(|(_, u)| u.created_by.as_deref() == Some("agent"))
            .map(|(name, usage)| SkillUsageReport::from((name, usage)))
            .collect();
        Ok(rows)
    }

    /// Get usage data for a specific skill.
    pub fn get(&self, name: &str) -> Option<SkillUsage> {
        self.load().ok()?.remove(name)
    }

    /// Load all usage data from the store.
    pub fn load(&self) -> Result<HashMap<String, SkillUsage>, std::io::Error> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let raw = fs::read_to_string(&self.path)?;
        if raw.trim().is_empty() {
            return Ok(HashMap::new());
        }
        serde_json::from_str(&raw).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })
    }

    /// Save all usage data to the store.
    pub fn save(&self, data: &HashMap<String, SkillUsage>) -> Result<(), std::io::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(data).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// Save a single entry (load existing, update, save).
    pub fn save_entry(&self, name: &str, entry: &SkillUsage) -> Result<(), std::io::Error> {
        let mut data = self.load().unwrap_or_default();
        data.insert(name.to_string(), entry.clone());
        self.save(&data)
    }

    fn update(&self, name: &str, f: impl FnOnce(&mut SkillUsage)) {
        match self.load() {
            Ok(mut data) => {
                let entry = data
                    .entry(name.to_string())
                    .or_insert_with(|| SkillUsage::new(name));
                f(entry);
                if let Err(e) = self.save(&data) {
                    debug!("SkillUsageStore: save failed for '{}': {}", name, e);
                }
            }
            Err(e) => {
                debug!("SkillUsageStore: load failed for '{}': {}", name, e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_skill_has_zero_counts() {
        let usage = SkillUsage::new("test-skill");
        assert_eq!(usage.use_count, 0);
        assert_eq!(usage.view_count, 0);
        assert_eq!(usage.patch_count, 0);
        assert_eq!(usage.state, Lifecycle::Active);
        assert!(!usage.pinned);
        assert!(usage.absorbed_into.is_none());
        assert!(usage.created_by.is_none());
    }

    #[test]
    fn bump_increments_counts() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("skill-a");
        store.bump_use("skill-a");
        store.bump_view("skill-a");

        let entry = store.get("skill-a").unwrap();
        assert_eq!(entry.use_count, 2);
        assert_eq!(entry.view_count, 1);
    }

    #[test]
    fn bump_sets_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("s1");
        let entry = store.get("s1").unwrap();
        assert!(entry.last_used_at.is_some());
        assert!(entry.last_activity_at().is_some());
    }

    #[test]
    fn bump_patch_increments() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_patch("s1");
        store.bump_patch("s1");
        store.bump_patch("s1");
        let entry = store.get("s1").unwrap();
        assert_eq!(entry.patch_count, 3);
        assert!(entry.last_patched_at.is_some());
    }

    #[test]
    fn mark_agent_created() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("agent-skill");
        store.mark_agent_created("agent-skill");

        assert!(store.is_agent_created("agent-skill"));
        let names = store.agent_created_names();
        assert!(names.contains(&"agent-skill".to_string()));
    }

    #[test]
    fn set_state_archived() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("s1");
        store.set_state("s1", Lifecycle::Archived);
        let entry = store.get("s1").unwrap();
        assert_eq!(entry.state, Lifecycle::Archived);
        assert!(entry.archived_at.is_some());
    }

    #[test]
    fn set_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("s1");
        store.set_pinned("s1", true);
        let entry = store.get("s1").unwrap();
        assert!(entry.pinned);
    }

    #[test]
    fn forget_removes_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("to-remove");
        store.forget("to-remove");
        assert!(store.get("to-remove").is_none());
    }

    #[test]
    fn forget_with_intent_records_absorbed_into() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("old-skill");
        store.forget_with_intent("old-skill", Some("new-skill"));

        assert!(store.get("old-skill").is_none());
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());
        assert!(store.get("nope").is_none());
    }

    #[test]
    fn load_empty_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());
        let data = store.load().unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn activity_count_sums_all() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("s1");
        store.bump_use("s1");
        store.bump_view("s1");
        store.bump_patch("s1");

        let entry = store.get("s1").unwrap();
        assert_eq!(entry.activity_count(), 4);
    }

    #[test]
    fn agent_created_report_filters() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());

        store.bump_use("manual-skill");
        store.bump_use("agent-skill");
        store.mark_agent_created("agent-skill");

        let report = store.agent_created_report().unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].name, "agent-skill");
    }
}