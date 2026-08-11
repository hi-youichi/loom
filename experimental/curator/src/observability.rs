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
        env_config::home::loom_home()
            .join("data")
            .join("observability")
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

    pub fn record_review(
        &self,
        skill_name: &str,
        memory_updates: usize,
        skill_updates: usize,
        duration_ms: u64,
    ) {
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

        entries.sort_by_key(|a| std::cmp::Reverse(a.timestamp));
        entries.truncate(limit);
        Ok(entries)
    }

    pub fn generate_report(&self) -> String {
        let entries = self.load_recent(1000).unwrap_or_default();
        let total_reviews = entries
            .iter()
            .filter(|e| matches!(e.event, EvolutionEvent::ReviewCompleted { .. }))
            .count();
        let total_curator = entries
            .iter()
            .filter(|e| matches!(e.event, EvolutionEvent::CuratorRun { .. }))
            .count();
        let total_evolutions = entries
            .iter()
            .filter(|e| matches!(e.event, EvolutionEvent::EvolutionAttempted { .. }))
            .count();
        let accepted = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.event,
                    EvolutionEvent::EvolutionAttempted { accepted: true, .. }
                )
            })
            .count();

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
            total_reviews,
            total_curator,
            total_evolutions,
            accepted,
            skills_created,
            skills_deleted,
            entries.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, ObservabilityStore) {
        let dir = TempDir::new().unwrap();
        let store = ObservabilityStore::new(dir.path());
        (dir, store)
    }

    fn fixed_entry(name: &str, event: EvolutionEvent) -> EvolutionTrackerEntry {
        EvolutionTrackerEntry {
            skill_name: name.to_string(),
            timestamp: Utc::now(),
            event,
        }
    }

    #[test]
    fn new_creates_base_dir() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("a/b/c");
        let _store = ObservabilityStore::new(&nested);
        assert!(nested.exists(), "new() should create the base dir tree");
    }

    #[test]
    fn default_path_ends_with_observability() {
        let p = ObservabilityStore::default_path();
        assert!(p.ends_with("observability"));
    }

    #[test]
    fn record_writes_jsonl_line_for_today() {
        let (_dir, store) = store();
        let entry = fixed_entry("skill-a", EvolutionEvent::SkillDeleted);
        store.record(&entry).unwrap();

        let date = entry.timestamp.format("%Y-%m-%d").to_string();
        let path = store.base_dir.join(format!("{}.jsonl", date));
        let content = std::fs::read_to_string(path).unwrap();
        assert_eq!(content.lines().count(), 1);

        let parsed: EvolutionTrackerEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed.skill_name, "skill-a");
        assert!(matches!(parsed.event, EvolutionEvent::SkillDeleted));
    }

    #[test]
    fn record_appends_to_existing_file() {
        let (_dir, store) = store();
        store
            .record(&fixed_entry("a", EvolutionEvent::SkillDeleted))
            .unwrap();
        store
            .record(&fixed_entry("b", EvolutionEvent::SkillDeleted))
            .unwrap();
        let files: Vec<_> = std::fs::read_dir(&store.base_dir).unwrap().collect();
        assert_eq!(files.len(), 1, "both records share the same day file");
    }

    #[test]
    fn record_review_logs_on_record_failure_path() {
        let (_dir, store) = store();

        let mut entry = fixed_entry("x", EvolutionEvent::SkillDeleted);

        entry.timestamp = Utc::now();

        store.record_review("x", 1, 2, 10);

        let any: Vec<_> = std::fs::read_dir(&store.base_dir).unwrap().collect();
        assert_eq!(any.len(), 1);
    }

    #[test]
    fn record_review_logs_when_record_errors() {
        let dir = TempDir::new().unwrap();

        let store = ObservabilityStore {
            base_dir: dir.path().join("does-not-exist-unreachable"),
        };

        store.record_review("x", 1, 2, 10);

        assert!(!store.base_dir.exists());
    }

    #[test]
    fn record_curator_writes_curator_run_event() {
        let (_dir, store) = store();
        store.record_curator(5, 2, 1, 0);

        let entries = store.load_recent(10).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            entries[0].event,
            EvolutionEvent::CuratorRun {
                active: 5,
                stale: 2,
                archived: 1,
                overlapping: 0
            }
        ));
        assert_eq!(entries[0].skill_name, "__curator__");
    }

    #[test]
    fn record_curator_logs_when_record_errors() {
        let dir = TempDir::new().unwrap();
        let store = ObservabilityStore {
            base_dir: dir.path().join("nested-missing"),
        };
        store.record_curator(1, 0, 0, 0);
        assert!(!store.base_dir.exists());
    }

    #[test]
    fn record_evolution_writes_evolution_attempted_event() {
        let (_dir, store) = store();
        store.record_evolution("s", 0.1, 0.9, true);

        let entries = store.load_recent(10).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            entries[0].event,
            EvolutionEvent::EvolutionAttempted {
                baseline_score: 0.1,
                evolved_score: 0.9,
                accepted: true
            }
        ));
    }

    #[test]
    fn record_evolution_logs_when_record_errors() {
        let dir = TempDir::new().unwrap();
        let store = ObservabilityStore {
            base_dir: dir.path().join("nested-missing"),
        };
        store.record_evolution("s", 0.0, 1.0, false);
        assert!(!store.base_dir.exists());
    }

    #[test]
    fn load_recent_empty_dir_returns_empty() {
        let (_dir, store) = store();
        assert!(store.load_recent(10).unwrap().is_empty());
    }

    #[test]
    fn load_recent_missing_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = ObservabilityStore {
            base_dir: dir.path().join("missing"),
        };
        assert!(store.load_recent(10).unwrap().is_empty());
    }

    #[test]
    fn load_recent_respects_limit_and_orders_descending() {
        let (_dir, store) = store();

        let mut entries = Vec::new();
        for i in 0..5 {
            let e = EvolutionTrackerEntry {
                skill_name: format!("s{}", i),
                timestamp: Utc::now() + chrono::Duration::milliseconds(i * 10),
                event: EvolutionEvent::SkillDeleted,
            };
            std::thread::sleep(std::time::Duration::from_millis(5));
            entries.push(e.clone());
            store.record(&e).unwrap();
        }

        let loaded = store.load_recent(3).unwrap();
        assert_eq!(loaded.len(), 3);

        assert!(loaded[0].timestamp >= loaded[1].timestamp);
        assert!(loaded[1].timestamp >= loaded[2].timestamp);
    }

    #[test]
    fn load_recent_skips_malformed_lines() {
        let (_dir, store) = store();

        let date = Utc::now().format("%Y-%m-%d").to_string();
        let path = store.base_dir.join(format!("{}.jsonl", date));
        std::fs::write(&path, "not-json\n").unwrap();

        store
            .record(&fixed_entry("good", EvolutionEvent::SkillDeleted))
            .unwrap();

        let loaded = store.load_recent(10).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].skill_name, "good");
    }

    #[test]
    fn load_recent_ignores_non_jsonl_files() {
        let (_dir, store) = store();
        std::fs::write(store.base_dir.join("readme.txt"), "hi").unwrap();
        std::fs::write(store.base_dir.join("2020-01-01.log"), "hi").unwrap();
        assert!(store.load_recent(10).unwrap().is_empty());
    }

    #[test]
    fn generate_report_empty_returns_zeros() {
        let (_dir, store) = store();
        let report = store.generate_report();
        assert!(report.contains("Reviews: 0"));
        assert!(report.contains("Curator runs: 0"));
        assert!(report.contains("Evolution attempts: 0 (accepted: 0)"));
        assert!(report.contains("Skills created: 0, deleted: 0"));
        assert!(report.contains("Total events: 0"));
    }

    #[test]
    fn generate_report_counts_each_event_type() {
        let (_dir, store) = store();

        store.record_review("a", 2, 1, 100);
        store.record_curator(3, 1, 0, 0);
        store.record_evolution("b", 0.2, 0.8, true);
        store.record_evolution("c", 0.2, 0.3, false);
        store
            .record(&fixed_entry(
                "d",
                EvolutionEvent::SkillCreated {
                    source: "review".into(),
                },
            ))
            .unwrap();
        store
            .record(&fixed_entry("e", EvolutionEvent::SkillDeleted))
            .unwrap();

        let report = store.generate_report();
        assert!(report.contains("Reviews: 1"));
        assert!(report.contains("Curator runs: 1"));
        assert!(report.contains("Evolution attempts: 2 (accepted: 1)"));
        assert!(report.contains("Skills created: 1, deleted: 1"));
        assert!(report.contains("Total events: 6"));
    }

    #[test]
    fn generate_report_fallback_when_load_errors() {
        let dir = TempDir::new().unwrap();

        let bad = dir.path().join("notdir.jsonl");
        std::fs::write(&bad, "x").unwrap();

        let store = ObservabilityStore { base_dir: bad };

        let report = store.generate_report();
        assert!(report.contains("Total events: 0"));
    }

    #[test]
    fn evolution_events_serialize_deserialize_roundtrip() {
        let events = vec![
            EvolutionEvent::ReviewCompleted {
                memory_updates: 1,
                skill_updates: 2,
                duration_ms: 50,
            },
            EvolutionEvent::CuratorRun {
                active: 1,
                stale: 0,
                archived: 0,
                overlapping: 0,
            },
            EvolutionEvent::EvolutionAttempted {
                baseline_score: 0.0,
                evolved_score: 1.0,
                accepted: true,
            },
            EvolutionEvent::SkillCreated {
                source: "test".into(),
            },
            EvolutionEvent::SkillDeleted,
            EvolutionEvent::MemoryUpdated {
                file: "mem.md".into(),
                action: "upsert".into(),
            },
        ];
        for ev in events {
            let entry = fixed_entry("rt", ev);
            let json = serde_json::to_string(&entry).unwrap();
            let back: EvolutionTrackerEntry = serde_json::from_str(&json).unwrap();
            assert_eq!(back.skill_name, "rt");
        }
    }
}
