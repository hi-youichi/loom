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

    /// Remove usage data for a skill.
    pub fn forget(&self, name: &str) {
        match self.load() {
            Ok(mut data) => {
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

    fn make_store() -> (tempfile::TempDir, SkillUsageStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillUsageStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn bump_use_increments_count() {
        let (_dir, store) = make_store();
        store.bump_use("test-skill");
        store.bump_use("test-skill");
        let usage = store.get("test-skill").unwrap();
        assert_eq!(usage.use_count, 2);
        assert!(usage.last_used_at.is_some());
    }

    #[test]
    fn bump_view_increments_count() {
        let (_dir, store) = make_store();
        store.bump_view("test-skill");
        let usage = store.get("test-skill").unwrap();
        assert_eq!(usage.view_count, 1);
    }

    #[test]
    fn bump_patch_increments_count() {
        let (_dir, store) = make_store();
        store.bump_patch("test-skill");
        let usage = store.get("test-skill").unwrap();
        assert_eq!(usage.patch_count, 1);
    }

    #[test]
    fn mark_agent_created() {
        let (_dir, store) = make_store();
        store.mark_agent_created("ai-skill");
        let usage = store.get("ai-skill").unwrap();
        assert_eq!(usage.created_by, Some("agent".to_string()));
    }

    #[test]
    fn set_state_archived() {
        let (_dir, store) = make_store();
        store.set_state("old-skill", Lifecycle::Archived);
        let usage = store.get("old-skill").unwrap();
        assert!(matches!(usage.state, Lifecycle::Archived));
        assert!(usage.archived_at.is_some());
    }

    #[test]
    fn set_pinned() {
        let (_dir, store) = make_store();
        store.set_pinned("favorite-skill", true);
        let usage = store.get("favorite-skill").unwrap();
        assert!(usage.pinned);
    }

    #[test]
    fn forget_removes_entry() {
        let (_dir, store) = make_store();
        store.bump_use("temp-skill");
        store.forget("temp-skill");
        assert!(store.get("temp-skill").is_none());
    }

    #[test]
    fn agent_created_report() {
        let (_dir, store) = make_store();
        store.mark_agent_created("ai-skill-1");
        store.mark_agent_created("ai-skill-2");
        store.bump_use("manual-skill");
        let report = store.agent_created_report().unwrap();
        assert_eq!(report.len(), 2);
        assert!(report.iter().all(|r| r.name.starts_with("ai-skill")));
    }

    #[test]
    fn skill_usage_activity_count() {
        let usage = SkillUsage::new("test");
        assert_eq!(usage.activity_count(), 0);
        
        let mut usage = usage;
        usage.use_count = 5;
        usage.view_count = 3;
        usage.patch_count = 2;
        assert_eq!(usage.activity_count(), 10);
    }

    #[test]
    fn skill_usage_last_activity_at() {
        let mut usage = SkillUsage::new("test");
        assert!(usage.last_activity_at().is_none());
        
        usage.last_used_at = Some("2024-01-01T00:00:00Z".to_string());
        usage.last_viewed_at = Some("2024-01-02T00:00:00Z".to_string());
        usage.last_patched_at = Some("2024-01-01T12:00:00Z".to_string());
        
        assert_eq!(usage.last_activity_at(), Some("2024-01-02T00:00:00Z".to_string()));
    }

    #[test]
    fn skill_usage_report_from_usage() {
        let usage = SkillUsage {
            name: "test-skill".to_string(),
            use_count: 10,
            view_count: 5,
            patch_count: 2,
            last_used_at: Some("2024-01-01T00:00:00Z".to_string()),
            last_viewed_at: None,
            last_patched_at: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            created_by: Some("agent".to_string()),
            state: Lifecycle::Active,
            pinned: false,
            archived_at: None,
        };
        let report = SkillUsageReport::from(("test-skill".to_string(), usage));
        assert_eq!(report.name, "test-skill");
        assert_eq!(report.activity_count, 17);
    }
}