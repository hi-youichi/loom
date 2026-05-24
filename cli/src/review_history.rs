use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub session_id: String,
    pub reviewed_at: DateTime<Utc>,
    pub trigger: String,
    pub model: String,
    pub memory_update_count: usize,
    pub skill_update_count: usize,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub duration_ms: u64,
}

pub struct ReviewHistory {
    path: PathBuf,
}

impl ReviewHistory {
    pub fn new(loom_home: &Path) -> Self {
        let dir = loom_home.join("data").join("review");
        let _ = fs::create_dir_all(&dir);
        Self {
            path: dir.join("history.jsonl"),
        }
    }

    pub fn append(&self, record: &ReviewRecord) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("Failed to open review history: {}", e))?;
        let line = serde_json::to_string(record)
            .map_err(|e| format!("Failed to serialize record: {}", e))?;
        writeln!(file, "{}", line)
            .map_err(|e| format!("Failed to write record: {}", e))?;
        Ok(())
    }

    pub fn list(&self, limit: usize) -> Result<Vec<ReviewRecord>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&self.path)
            .map_err(|e| format!("Failed to open history: {}", e))?;
        let reader = std::io::BufReader::new(file);
        let records: Vec<ReviewRecord> = reader
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str(&line).ok())
            .collect();
        let start = records.len().saturating_sub(limit);
        Ok(records[start..].to_vec())
    }

    pub fn reviewed_session_ids(&self) -> Result<HashSet<String>, String> {
        let records = self.list(usize::MAX)?;
        Ok(records
            .into_iter()
            .filter(|r| !r.skipped)
            .map(|r| r.session_id)
            .collect())
    }

    pub fn find_by_session(&self, session_id: &str) -> Result<Option<ReviewRecord>, String> {
        let records = self.list(usize::MAX)?;
        Ok(records
            .into_iter()
            .rev()
            .find(|r| r.session_id == session_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, ReviewHistory) {
        let dir = TempDir::new().unwrap();
        let history = ReviewHistory::new(dir.path());
        (dir, history)
    }

    fn make_record(session_id: &str, skipped: bool) -> ReviewRecord {
        ReviewRecord {
            session_id: session_id.to_string(),
            reviewed_at: Utc::now(),
            trigger: "manual".to_string(),
            model: "gpt-4o-mini".to_string(),
            memory_update_count: 1,
            skill_update_count: 0,
            skipped,
            skip_reason: if skipped {
                Some("too_short".to_string())
            } else {
                None
            },
            duration_ms: 100,
        }
    }

    #[test]
    fn test_append_and_list() {
        let (_dir, history) = setup();
        history.append(&make_record("s1", false)).unwrap();
        history.append(&make_record("s2", false)).unwrap();
        let records = history.list(10).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].session_id, "s1");
        assert_eq!(records[1].session_id, "s2");
    }

    #[test]
    fn test_list_with_limit() {
        let (_dir, history) = setup();
        for i in 0..5 {
            history
                .append(&make_record(&format!("s{}", i), false))
                .unwrap();
        }
        let records = history.list(3).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].session_id, "s2");
    }

    #[test]
    fn test_reviewed_session_ids_skips_failed() {
        let (_dir, history) = setup();
        history.append(&make_record("s1", false)).unwrap();
        history.append(&make_record("s2", true)).unwrap();
        history.append(&make_record("s3", false)).unwrap();
        let ids = history.reviewed_session_ids().unwrap();
        assert!(ids.contains("s1"));
        assert!(!ids.contains("s2"));
        assert!(ids.contains("s3"));
    }

    #[test]
    fn test_find_by_session() {
        let (_dir, history) = setup();
        history.append(&make_record("s1", false)).unwrap();
        assert!(history.find_by_session("s1").unwrap().is_some());
        assert!(history.find_by_session("s99").unwrap().is_none());
    }
}
