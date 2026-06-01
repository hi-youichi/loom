use super::skill_registry::Lifecycle;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

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

    pub fn activity_count(&self) -> u64 {
        self.use_count + self.view_count + self.patch_count
    }
}

pub struct SkillUsageStore {
    path: PathBuf,
}

impl SkillUsageStore {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            path: base_dir.join(".usage.json"),
        }
    }

    pub fn bump_use(&self, name: &str) {
        self.update(name, |u| {
            u.use_count += 1;
            u.last_used_at = Some(Utc::now().to_rfc3339());
        });
    }

    pub fn bump_view(&self, name: &str) {
        self.update(name, |u| {
            u.view_count += 1;
            u.last_viewed_at = Some(Utc::now().to_rfc3339());
        });
    }

    pub fn bump_patch(&self, name: &str) {
        self.update(name, |u| {
            u.patch_count += 1;
            u.last_patched_at = Some(Utc::now().to_rfc3339());
        });
    }

    pub fn mark_agent_created(&self, name: &str) {
        self.update(name, |u| {
            u.created_by = Some("agent".to_string());
        });
    }

    pub fn set_state(&self, name: &str, state: Lifecycle) {
        self.update(name, |u| {
            u.state = state;
            if matches!(state, Lifecycle::Archived) {
                u.archived_at = Some(Utc::now().to_rfc3339());
            }
        });
    }

    pub fn set_pinned(&self, name: &str, pinned: bool) {
        self.update(name, |u| {
            u.pinned = pinned;
        });
    }

    pub fn forget(&self, name: &str) {
        match self.load() {
            Ok(mut data) => {
                data.remove(name);
                let _ = self.save(&data);
            }
            Err(e) => {
                debug!("SkillUsageStore: forget '{}' failed to load: {}", name, e);
            }
        }
    }

    pub fn agent_created_report(&self) -> Result<Vec<SkillUsageReport>, String> {
        let data = self.load().map_err(|e| e.to_string())?;
        let rows: Vec<SkillUsageReport> = data
            .into_iter()
            .filter(|(_, u)| u.created_by.as_deref() == Some("agent"))
            .map(|(name, usage)| {
                let last_activity_at = usage.last_activity_at();
                let activity_count = usage.activity_count();
                SkillUsageReport {
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
            })
            .collect();
        Ok(rows)
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
}

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
