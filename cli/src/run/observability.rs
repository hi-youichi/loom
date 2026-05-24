use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionTrackerEntry {
    pub skill_name: String,
    pub timestamp: DateTime<Utc>,
    pub event: EvolutionEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvolutionEvent {
    ReviewCompleted {
        memory_updates: usize,
        skill_updates: usize,
        duration_ms: u64,
    },
    CuratorRun {
        active: usize,
        stale: usize,
        archived: usize,
        overlapping: usize,
    },
    EvolutionAttempted {
        baseline_score: f64,
        evolved_score: f64,
        accepted: bool,
    },
    SkillCreated {
        source: String,
    },
    SkillDeleted,
    MemoryUpdated {
        file: String,
        action: String,
    },
}

pub struct ObservabilityStore {
    base_dir: PathBuf,
}

impl ObservabilityStore {
    pub fn new(base_dir: &Path) -> Self {
        let _ = fs::create_dir_all(base_dir);
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    pub fn default_path() -> PathBuf {
        config::home::loom_home().join("data").join("observability")
    }

    pub fn record(&self, entry: &EvolutionTrackerEntry) -> Result<(), String> {
        let date = entry.timestamp.format("%Y-%m-%d").to_string();
        let path = self.base_dir.join(format!("{}.jsonl", date));
        let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        writeln!(file, "{}", line).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn record_review(&self, skill_name: &str, memory_updates: usize, skill_updates: usize, duration_ms: u64) {
        let entry = EvolutionTrackerEntry {
            skill_name: skill_name.to_string(),
            timestamp: Utc::now(),
            event: EvolutionEvent::ReviewCompleted {
                memory_updates,
                skill_updates,
                duration_ms,
            },
        };
        if let Err(e) = self.record(&entry) {
            info!("Failed to record review event: {}", e);
        }
    }

    pub fn record_curator(&self, active: usize, stale: usize, archived: usize, overlapping: usize) {
        let entry = EvolutionTrackerEntry {
            skill_name: "__curator__".to_string(),
            timestamp: Utc::now(),
            event: EvolutionEvent::CuratorRun {
                active,
                stale,
                archived,
                overlapping,
            },
        };
        if let Err(e) = self.record(&entry) {
            info!("Failed to record curator event: {}", e);
        }
    }

    pub fn record_evolution(&self, skill_name: &str, baseline: f64, evolved: f64, accepted: bool) {
        let entry = EvolutionTrackerEntry {
            skill_name: skill_name.to_string(),
            timestamp: Utc::now(),
            event: EvolutionEvent::EvolutionAttempted {
                baseline_score: baseline,
                evolved_score: evolved,
                accepted,
            },
        };
        if let Err(e) = self.record(&entry) {
            info!("Failed to record evolution event: {}", e);
        }
    }

    pub fn load_recent(&self, limit: usize) -> Result<Vec<EvolutionTrackerEntry>, String> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut files: Vec<PathBuf> = fs::read_dir(&self.base_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|ext| ext == "jsonl").unwrap_or(false))
            .collect();
        files.sort();
        files.reverse();

        let mut entries = Vec::new();
        for file in files {
            if entries.len() >= limit {
                break;
            }
            let content = fs::read_to_string(&file).map_err(|e| e.to_string())?;
            for line in content.lines().rev() {
                if entries.len() >= limit {
                    break;
                }
                if let Ok(entry) = serde_json::from_str::<EvolutionTrackerEntry>(line) {
                    entries.push(entry);
                }
            }
        }

        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        entries.truncate(limit);
        Ok(entries)
    }

    pub fn generate_report(&self) -> String {
        let entries = self.load_recent(1000).unwrap_or_default();
        let total_reviews = entries.iter().filter(|e| matches!(e.event, EvolutionEvent::ReviewCompleted { .. })).count();
        let total_curator = entries.iter().filter(|e| matches!(e.event, EvolutionEvent::CuratorRun { .. })).count();
        let total_evolutions = entries.iter().filter(|e| matches!(e.event, EvolutionEvent::EvolutionAttempted { .. })).count();
        let accepted = entries.iter().filter(|e| matches!(e.event, EvolutionEvent::EvolutionAttempted { accepted: true, .. })).count();

        let mut skills_created = 0usize;
        let mut skills_deleted = 0usize;
        for entry in &entries {
            match &entry.event {
                EvolutionEvent::SkillCreated { .. } => skills_created += 1,
                EvolutionEvent::SkillDeleted => skills_deleted += 1,
                _ => {}
            }
        }

        format!(
            "Evolution Report\n\
            ================\n\
            Reviews: {}\n\
            Curator runs: {}\n\
            Evolution attempts: {} (accepted: {})\n\
            Skills created: {}, deleted: {}\n\
            Total events: {}",
            total_reviews, total_curator, total_evolutions, accepted,
            skills_created, skills_deleted, entries.len()
        )
    }
}
