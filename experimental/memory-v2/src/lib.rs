use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tracing::info;

use fs4::fs_std::FileExt;

const ENTRY_DELIMITER: &str = "\n§\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryFile {
    User,
    Project,
}

impl MemoryFile {
    pub fn filename(&self) -> &'static str {
        match self {
            MemoryFile::User => "USER.md",
            MemoryFile::Project => "PROJECT.md",
        }
    }

    pub fn char_limit(&self) -> usize {
        match self {
            MemoryFile::User => 4000,
            MemoryFile::Project => 8000,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            MemoryFile::User => "USER (who the user is)",
            MemoryFile::Project => "PROJECT (agent notes + persistent knowledge)",
        }
    }

    pub fn all() -> &'static [MemoryFile] {
        &[MemoryFile::Project, MemoryFile::User]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryProvenance {
    pub write_origin: String,
    pub execution_context: String,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
}

impl MemoryProvenance {
    pub fn foreground_default() -> Self {
        Self {
            write_origin: "assistant_tool".into(),
            execution_context: "foreground".into(),
            session_id: None,
            parent_session_id: None,
        }
    }

    pub fn background_review(
        session_id: impl Into<String>,
        parent_session_id: impl Into<String>,
    ) -> Self {
        let session_id = session_id.into();
        let parent_session_id = parent_session_id.into();
        assert!(
            !session_id.trim().is_empty(),
            "background_review session_id must not be empty"
        );
        assert!(
            !parent_session_id.trim().is_empty(),
            "background_review parent_session_id must not be empty"
        );
        Self {
            write_origin: "background_review".into(),
            execution_context: "background_review".into(),
            session_id: Some(session_id),
            parent_session_id: Some(parent_session_id),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMatch {
    pub file: MemoryFile,
    pub line_number: usize,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_max_memory_chars")]
    pub max_memory_chars: usize,
}

fn default_max_memory_chars() -> usize {
    8000
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_memory_chars: default_max_memory_chars(),
        }
    }
}

struct MemorySnapshot {
    text: String,
    captured: bool,
}

pub struct MemoryStore {
    base_dir: PathBuf,
    config: MemoryConfig,
    snapshot: RwLock<MemorySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddResult {
    pub success: bool,
    pub message: String,
    pub entry_count: usize,
    pub usage: String,
    pub provenance: MemoryProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceResult {
    pub success: bool,
    pub message: String,
    pub matched_count: usize,
    pub entry_count: usize,
    pub usage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveResult {
    pub success: bool,
    pub message: String,
    pub entry_count: usize,
    pub usage: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("content cannot be empty")]
    EmptyContent,
    #[error("duplicate entry")]
    Duplicate,
    #[error("would exceed capacity: {0}")]
    CapacityExceeded(String),
    #[error("no matching entry found")]
    NotFound,
    #[error("ambiguous match: {0} entries match, provide more specific old_text")]
    AmbiguousMatch(usize),
    #[error("drift detected: {0}")]
    DriftDetected(String),
}

impl MemoryStore {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
            config: MemoryConfig::default(),
            snapshot: RwLock::new(MemorySnapshot {
                text: String::new(),
                captured: false,
            }),
        }
    }

    pub fn with_config(base_dir: &Path, config: MemoryConfig) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
            config,
            snapshot: RwLock::new(MemorySnapshot {
                text: String::new(),
                captured: false,
            }),
        }
    }

    pub fn default_path() -> PathBuf {
        env_config::home::loom_home().join("data").join("memory")
    }

    pub fn base_dir(&self) -> PathBuf {
        self.base_dir.clone()
    }

    fn file_path(&self, file: MemoryFile) -> PathBuf {
        self.base_dir.join(file.filename())
    }

    fn facts_path(&self) -> PathBuf {
        self.base_dir.join("FACTS.md")
    }

    fn ensure_dir(&self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.base_dir)?;
        Ok(())
    }

    // ── Snapshot ──────────────────────────────────────

    pub fn capture_snapshot(&self) -> Result<String, MemoryError> {
        self.ensure_dir()?;
        self.migrate_facts_into_project()?;

        let mut snap = self.snapshot.write().unwrap();
        if snap.captured {
            return Ok(snap.text.clone());
        }

        let mut blocks = Vec::new();
        let mut total_len = 0;

        for &file in MemoryFile::all() {
            let entries = self.read_file_entries(file)?;
            if entries.is_empty() {
                continue;
            }
            let deduped = dedup_entries(&entries);
            let body = deduped.join(ENTRY_DELIMITER);
            let usage_pct = (body.len() * 100 / file.char_limit()).min(999);
            let header = format!(
                "══ {} [{}% — {}/{} chars] ══",
                file.label(),
                usage_pct,
                body.len(),
                file.char_limit(),
            );
            let block = format!("{}\n{}", header, body);
            if total_len + block.len() <= self.config.max_memory_chars {
                total_len += block.len();
                blocks.push(block);
            } else if blocks.is_empty() {
                let truncated: String = body.chars().take(self.config.max_memory_chars).collect();
                blocks.push(format!("{}\n{}", header, truncated));
                break;
            }
        }

        snap.text = blocks.join("\n\n");
        snap.captured = true;
        Ok(snap.text.clone())
    }

    pub fn snapshot_text(&self) -> Result<String, MemoryError> {
        {
            let snap = self.snapshot.read().unwrap();
            if snap.captured {
                return Ok(snap.text.clone());
            }
        }
        self.capture_snapshot()
    }

    // ── Entry-level operations ────────────────────────

    pub fn add_entry(
        &self,
        file: MemoryFile,
        content: &str,
        provenance: &MemoryProvenance,
    ) -> Result<AddResult, MemoryError> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(MemoryError::EmptyContent);
        }

        info!(
            write_origin = %provenance.write_origin,
            execution_context = %provenance.execution_context,
            "Memory add to {:?}",
            file
        );

        self.ensure_dir()?;
        let _lock = self.acquire_file_lock(file)?;
        let mut entries = self.read_file_entries(file)?;

        if entries.iter().any(|e| e.trim() == trimmed) {
            return Ok(AddResult {
                success: true,
                message: "Entry already exists, skipping duplicate".into(),
                entry_count: entries.len(),
                usage: self.fmt_usage(file, &entries),
                provenance: provenance.clone(),
            });
        }

        let test_total = entries
            .iter()
            .map(|e| e.len())
            .sum::<usize>()
            + ENTRY_DELIMITER.len()
            + trimmed.len();
        if test_total > file.char_limit() {
            return Err(MemoryError::CapacityExceeded(self.fmt_usage(file, &entries)));
        }

        entries.push(trimmed.to_string());
        self.write_file_entries_atomic(file, &entries)?;

        Ok(AddResult {
            success: true,
            message: "Entry added".into(),
            entry_count: entries.len(),
            usage: self.fmt_usage(file, &entries),
            provenance: provenance.clone(),
        })
    }

    pub fn replace_entry(
        &self,
        file: MemoryFile,
        old_text: &str,
        new_content: &str,
    ) -> Result<ReplaceResult, MemoryError> {
        let old_trimmed = old_text.trim();
        let new_trimmed = new_content.trim();
        if old_trimmed.is_empty() || new_trimmed.is_empty() {
            return Err(MemoryError::EmptyContent);
        }

        self.ensure_dir()?;
        let _lock = self.acquire_file_lock(file)?;
        let mut entries = self.read_file_entries(file)?;

        let indices: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.contains(old_trimmed))
            .map(|(i, _)| i)
            .collect();

        match indices.len() {
            0 => return Err(MemoryError::NotFound),
            1 => {
                entries[indices[0]] = new_trimmed.to_string();
            }
            _ => {
                let all_same = indices
                    .iter()
                    .all(|&i| entries[i] == entries[indices[0]]);
                if all_same {
                    entries[indices[0]] = new_trimmed.to_string();
                } else {
                    return Err(MemoryError::AmbiguousMatch(indices.len()));
                }
            }
        }

        let joined = entries.join(ENTRY_DELIMITER);
        if joined.len() > file.char_limit() {
            return Err(MemoryError::CapacityExceeded(self.fmt_usage(file, &entries)));
        }

        self.write_file_entries_atomic(file, &entries)?;

        Ok(ReplaceResult {
            success: true,
            message: "Entry replaced".into(),
            matched_count: indices.len(),
            entry_count: entries.len(),
            usage: self.fmt_usage(file, &entries),
        })
    }

    pub fn remove_entry(
        &self,
        file: MemoryFile,
        old_text: &str,
    ) -> Result<RemoveResult, MemoryError> {
        let old_trimmed = old_text.trim();
        if old_trimmed.is_empty() {
            return Err(MemoryError::EmptyContent);
        }

        self.ensure_dir()?;
        let _lock = self.acquire_file_lock(file)?;
        let mut entries = self.read_file_entries(file)?;

        let indices: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.contains(old_trimmed))
            .map(|(i, _)| i)
            .collect();

        match indices.len() {
            0 => return Err(MemoryError::NotFound),
            1 => {
                entries.remove(indices[0]);
            }
            _ => {
                let all_same = indices
                    .iter()
                    .all(|&i| entries[i] == entries[indices[0]]);
                if all_same {
                    entries.remove(indices[0]);
                } else {
                    return Err(MemoryError::AmbiguousMatch(indices.len()));
                }
            }
        }

        self.write_file_entries_atomic(file, &entries)?;

        Ok(RemoveResult {
            success: true,
            message: "Entry removed".into(),
            entry_count: entries.len(),
            usage: self.fmt_usage(file, &entries),
        })
    }

    pub fn read_entries(&self, file: MemoryFile) -> Result<Vec<String>, MemoryError> {
        self.read_file_entries(file)
    }

    // ── Legacy API (backward compat) ──────────────────

    pub fn load(&self, file: MemoryFile) -> Result<String, MemoryError> {
        let path = self.file_path(file);
        if !path.exists() {
            return Ok(String::new());
        }
        Ok(fs::read_to_string(&path)?)
    }

    pub fn load_all_for_prompt(&self) -> Result<String, MemoryError> {
        match self.snapshot_text() {
            Ok(text) if !text.is_empty() => Ok(text),
            _ => {
                let mut parts = Vec::new();
                for &file in MemoryFile::all() {
                    let content = self.load(file)?;
                    if !content.trim().is_empty() {
                        parts.push(content);
                    }
                }
                Ok(parts.join("\n\n"))
            }
        }
    }

    pub fn search(&self, query: &str) -> Result<Vec<MemoryMatch>, MemoryError> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        for &file in MemoryFile::all() {
            let content = self.load(file)?;
            for (i, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&query_lower) {
                    results.push(MemoryMatch {
                        file,
                        line_number: i + 1,
                        line: line.to_string(),
                    });
                }
            }
        }
        Ok(results)
    }

    // ── Internal: file I/O ────────────────────────────

    fn read_file_entries(&self, file: MemoryFile) -> Result<Vec<String>, MemoryError> {
        let path = self.file_path(file);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let raw = fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }

        if raw.contains('§') {
            return Ok(parse_delimited(&raw));
        }

        let entries = parse_legacy(&raw);
        if !entries.is_empty() {
            let _ = self.write_file_entries_atomic(file, &entries);
        }
        Ok(entries)
    }

    fn write_file_entries_atomic(
        &self,
        file: MemoryFile,
        entries: &[String],
    ) -> Result<(), MemoryError> {
        self.ensure_dir()?;
        let path = self.file_path(file);
        if entries.is_empty() {
            if path.exists() {
                fs::remove_file(&path)?;
            }
            return Ok(());
        }
        let content = entries.join(ENTRY_DELIMITER);
        atomic_write(&path, &content)
    }

    fn fmt_usage(&self, file: MemoryFile, entries: &[String]) -> String {
        let used = entries.join(ENTRY_DELIMITER).len();
        let limit = file.char_limit();
        let pct = (used * 100 / limit).min(999);
        format!("{}% — {}/{} chars", pct, used, limit)
    }

    // ── Internal: file lock ───────────────────────────

    fn acquire_file_lock(&self, file: MemoryFile) -> Result<FileLockGuard, MemoryError> {
        let lock_path = self.file_path(file).with_extension("md.lock");
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        f.lock_exclusive()
            .map_err(|e| MemoryError::Io(std::io::Error::other(e.to_string())))?;
        Ok(FileLockGuard { _file: f })
    }

    // ── Internal: FACTS.md migration ──────────────────

    fn migrate_facts_into_project(&self) -> Result<(), MemoryError> {
        let facts_path = self.facts_path();
        if !facts_path.exists() {
            return Ok(());
        }

        let facts_raw = fs::read_to_string(&facts_path)?;
        if facts_raw.trim().is_empty() {
            let _ = fs::remove_file(&facts_path);
            return Ok(());
        }

        let facts_entries = if facts_raw.contains('§') {
            parse_delimited(&facts_raw)
        } else {
            parse_legacy(&facts_raw)
        };

        let project_entries = self.read_file_entries(MemoryFile::Project)?;

        let mut merged = project_entries;
        merged.extend(facts_entries);
        merged = dedup_entries(&merged);

        self.ensure_dir()?;
        self.write_file_entries_atomic(MemoryFile::Project, &merged)?;

        let _ = fs::remove_file(&facts_path);
        info!("Migrated FACTS.md into PROJECT.md ({} entries total)", merged.len());

        Ok(())
    }
}

// ── File lock guard ───────────────────────────────────

struct FileLockGuard {
    _file: fs::File,
}

impl Drop for FileLockGuard {
    fn drop(&mut self) {}
}

// ── Atomic write ──────────────────────────────────────

fn atomic_write(path: &Path, content: &str) -> Result<(), MemoryError> {
    let parent = path
        .parent()
        .ok_or_else(|| MemoryError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory")))?;

    let temp_name = format!(".mem_{}.tmp", uuid::Uuid::new_v4());
    let temp_path = parent.join(&temp_name);

    let mut f = fs::File::create(&temp_path)?;
    f.write_all(content.as_bytes())?;
    f.flush()?;
    f.sync_all()?;
    drop(f);

    if let Err(e) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(MemoryError::Io(e));
    }

    Ok(())
}

// ── Parsing helpers ───────────────────────────────────

fn parse_delimited(raw: &str) -> Vec<String> {
    raw.split(ENTRY_DELIMITER)
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect()
}

fn parse_legacy(raw: &str) -> Vec<String> {
    let separator = if raw.contains("\n---\n") {
        "\n---\n"
    } else {
        "\n\n"
    };
    raw.split(separator)
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect()
}

fn dedup_entries(entries: &[String]) -> Vec<String> {
    let mut seen = Vec::new();
    for e in entries {
        if !seen.contains(e) {
            seen.push(e.clone());
        }
    }
    seen
}

// ── Tests ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fg() -> MemoryProvenance {
        MemoryProvenance::foreground_default()
    }

    #[test]
    fn add_and_read_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "prefers Rust", &fg())
            .unwrap();
        store.add_entry(MemoryFile::User, "uses vim", &fg()).unwrap();
        let entries = store.read_entries(MemoryFile::User).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], "prefers Rust");
        assert_eq!(entries[1], "uses vim");
    }

    #[test]
    fn add_deduplicates() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "likes coffee", &fg())
            .unwrap();
        let result = store
            .add_entry(MemoryFile::User, "likes coffee", &fg())
            .unwrap();
        assert!(result.message.contains("duplicate"));
        assert_eq!(store.read_entries(MemoryFile::User).unwrap().len(), 1);
    }

    #[test]
    fn replace_entry_by_substring() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "likes tea", &fg())
            .unwrap();
        store
            .replace_entry(MemoryFile::User, "tea", "coffee")
            .unwrap();
        let entries = store.read_entries(MemoryFile::User).unwrap();
        assert_eq!(entries[0], "coffee");
    }

    #[test]
    fn replace_rejects_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "likes Rust fast", &fg())
            .unwrap();
        store
            .add_entry(MemoryFile::User, "likes Rust safe", &fg())
            .unwrap();
        let result = store.replace_entry(MemoryFile::User, "Rust", "C++");
        assert!(matches!(result, Err(MemoryError::AmbiguousMatch(2))));
    }

    #[test]
    fn remove_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "entry A", &fg())
            .unwrap();
        store
            .add_entry(MemoryFile::User, "entry B", &fg())
            .unwrap();
        store
            .remove_entry(MemoryFile::User, "entry A")
            .unwrap();
        let entries = store.read_entries(MemoryFile::User).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "entry B");
    }

    #[test]
    fn remove_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "exists", &fg())
            .unwrap();
        let result = store.remove_entry(MemoryFile::User, "nonexistent");
        assert!(matches!(result, Err(MemoryError::NotFound)));
    }

    #[test]
    fn capacity_limit() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        let big = "x".repeat(3990);
        store.add_entry(MemoryFile::User, &big, &fg()).unwrap();
        let result = store.add_entry(MemoryFile::User, &"y".repeat(50), &fg());
        assert!(matches!(result, Err(MemoryError::CapacityExceeded(_))));
    }

    #[test]
    fn entry_delimiter_in_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::Project, "entry 1", &fg())
            .unwrap();
        store
            .add_entry(MemoryFile::Project, "entry 2", &fg())
            .unwrap();
        let raw = fs::read_to_string(dir.path().join("PROJECT.md")).unwrap();
        assert!(raw.contains('§'));
    }

    #[test]
    fn legacy_format_migration() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("USER.md"), "alpha\n\nbeta\n\ngamma").unwrap();
        let store = MemoryStore::new(dir.path());
        let entries = store.read_entries(MemoryFile::User).unwrap();
        assert_eq!(entries, vec!["alpha", "beta", "gamma"]);
        let raw = fs::read_to_string(dir.path().join("USER.md")).unwrap();
        assert!(raw.contains('§'));
    }

    #[test]
    fn facts_migration() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("FACTS.md"), "fact A\n\nfact B").unwrap();
        fs::write(dir.path().join("PROJECT.md"), "project X").unwrap();
        let store = MemoryStore::new(dir.path());
        store.capture_snapshot().unwrap();
        assert!(!dir.path().join("FACTS.md").exists());
        let entries = store.read_entries(MemoryFile::Project).unwrap();
        assert!(entries.contains(&"project X".to_string()));
        assert!(entries.contains(&"fact A".to_string()));
        assert!(entries.contains(&"fact B".to_string()));
    }

    #[test]
    fn snapshot_frozen() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "before snapshot", &fg())
            .unwrap();
        let snap1 = store.capture_snapshot().unwrap();
        store
            .add_entry(MemoryFile::User, "after snapshot", &fg())
            .unwrap();
        let snap2 = store.snapshot_text().unwrap();
        assert_eq!(snap1, snap2);
        assert!(snap1.contains("before snapshot"));
        assert!(!snap1.contains("after snapshot"));
    }

    #[test]
    fn snapshot_format() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "user entry", &fg())
            .unwrap();
        store
            .add_entry(MemoryFile::Project, "project entry", &fg())
            .unwrap();
        let snap = store.capture_snapshot().unwrap();
        assert!(snap.contains("PROJECT"));
        assert!(snap.contains("USER"));
        assert!(snap.contains("project entry"));
        assert!(snap.contains("user entry"));
    }

    #[test]
    fn search_finds_matches() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "likes Rust\nhates bugs", &fg())
            .unwrap();
        let matches = store.search("rust").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].file, MemoryFile::User);
    }

    #[test]
    fn load_backward_compat() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "hello", &fg())
            .unwrap();
        let content = store.load(MemoryFile::User).unwrap();
        assert!(content.contains("hello"));
    }

    #[test]
    fn atomic_write_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn empty_file_returns_empty_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        let entries = store.read_entries(MemoryFile::User).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn add_rejects_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        let result = store.add_entry(MemoryFile::User, "", &fg());
        assert!(matches!(result, Err(MemoryError::EmptyContent)));
    }

    #[test]
    fn replace_same_text_multiple_copies() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store
            .add_entry(MemoryFile::User, "identical", &fg())
            .unwrap();
        store
            .add_entry(MemoryFile::User, "identical", &fg())
            .unwrap();
        let result = store
            .add_entry(MemoryFile::User, "identical", &fg())
            .unwrap();
        assert!(result.message.contains("duplicate"));
        store
            .replace_entry(MemoryFile::User, "identical", "replaced")
            .unwrap();
        let entries = store.read_entries(MemoryFile::User).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "replaced");
    }
}
