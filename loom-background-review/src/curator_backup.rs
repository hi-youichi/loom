//! Curator Snapshot & Rollback — Phase 4
//!
//! Reference Hermes `curator_backup.py` (693 lines) core safety capabilities:
//! - `snapshot()` — execute pre-backup of skills directory as tar.gz
//! - `rollback()` — rollback from snapshot
//! - `list_snapshots()` — list available snapshots
//! - `prune_old_snapshots()` — clean old snapshots (keep recent N)

use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use tar::{Archive, Builder};
use thiserror::Error;

// ─────────────────────────────────────────────────────────────────────────────
// Error
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("skills directory not found: {0}")]
    SkillsDirNotFound(PathBuf),
    #[error("snapshot not found: {0}")]
    SnapshotNotFound(String),
    #[error("snapshot directory init failed: {0}")]
    BackupDirInitFailed(PathBuf),
}

pub type Result<T> = std::result::Result<T, BackupError>;

// ─────────────────────────────────────────────────────────────────────────────
// Snapshot metadata
// ─────────────────────────────────────────────────────────────────────────────

/// Snapshot metadata (stored in snapshot tar.gz as metadata.json)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotMeta {
    pub timestamp: String,
    pub skills_count: usize,
    pub size_bytes: u64,
    pub description: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// CuratorBackup
// ─────────────────────────────────────────────────────────────────────────────

/// Curator snapshot and rollback manager
///
/// Corresponds to Hermes `CuratorBackup` class, provides:
/// - Automatic snapshot (before curator modifies skills)
/// - Manual/auto rollback
/// - Snapshot list and cleanup
#[derive(Debug, Clone)]
pub struct CuratorBackup {
    /// Snapshot storage directory (default `~/.loom/backups/`)
    backup_dir: PathBuf,
}

impl CuratorBackup {
    /// Use default backup directory (`~/.loom/backups/`)
    pub fn new() -> Self {
        let backup_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("loom")
            .join("backups");
        Self { backup_dir }
    }

    /// Use custom backup directory
    pub fn with_backup_dir(mut self, backup_dir: PathBuf) -> Self {
        self.backup_dir = backup_dir;
        self
    }

    /// Snapshot directory
    pub fn backup_dir(&self) -> &Path {
        &self.backup_dir
    }

    /// Create snapshot of skills directory (tar.gz)
    ///
    /// Returns snapshot filename (without path), e.g. `curator-2025-08-19T12-34-56.tar.gz`
    ///
    /// # Arguments
    /// * `skills_dir` — `.loom/skills/` directory
    /// * `description` — optional snapshot description
    pub fn snapshot(&self, skills_dir: &Path, description: Option<&str>) -> Result<String> {
        if !skills_dir.exists() {
            return Err(BackupError::SkillsDirNotFound(skills_dir.to_path_buf()));
        }

        fs::create_dir_all(&self.backup_dir)
            .map_err(|_| BackupError::BackupDirInitFailed(self.backup_dir.clone()))?;

        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S%.9f").to_string();
        let filename = format!("curator-{}.tar.gz", timestamp);
        let filepath = self.backup_dir.join(&filename);

        let file = File::create(&filepath)?;
        let encoder = GzEncoder::new(file, flate2::Compression::default());
        let mut builder = Builder::new(encoder);

        // Collect skills count
        let skills_count = walkdir::WalkDir::new(skills_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count();

        // Pack skills_dir (without backup_dir itself)
        builder.append_dir_all("skills", skills_dir)?;

        // Write metadata.json (using append_data, requires tar 0.4.13+ Header::set_path)
        let meta = SnapshotMeta {
            timestamp: timestamp.clone(),
            skills_count,
            size_bytes: fs::metadata(&filepath)?.len(),
            description: description.map(|s| s.to_string()),
        };
        let meta_json = serde_json::to_string(&meta).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(meta_json.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::file());
        header.set_path("metadata.json")
            .map_err(std::io::Error::other)?;
        builder.append_data(&mut header, "metadata.json", meta_json.as_bytes())?;

        builder.finish()?;
        drop(builder);

        tracing::info!(
            "Curator snapshot created: {} ({} skills, {} bytes)",
            filename,
            skills_count,
            meta.size_bytes
        );

        Ok(filename)
    }

    /// List all snapshots (reverse time order)
    pub fn list_snapshots(&self) -> Result<Vec<SnapshotMeta>> {
        if !self.backup_dir.exists() {
            return Ok(vec![]);
        }

        let mut snapshots: Vec<SnapshotMeta> = Vec::new();

        for entry in fs::read_dir(&self.backup_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "gz").unwrap_or(false)
                && path.file_stem().map(|s| s.to_string_lossy().starts_with("curator-")).unwrap_or(false)
            {
                if let Ok(file) = File::open(&path) {
                    let decoder = GzDecoder::new(file);
                    let mut archive = Archive::new(decoder);
                    let meta_entry = archive.entries()?
                        .filter_map(|e| e.ok())
                        .find(|e| e.path().map(|p| p.to_string_lossy() == "metadata.json").unwrap_or(false));

                    if let Some(mut entry) = meta_entry {
                        let mut content = String::new();
                        if entry.read_to_string(&mut content).is_ok() {
                            if let Ok(meta) = serde_json::from_str::<SnapshotMeta>(&content) {
                                snapshots.push(meta);
                            }
                        }
                    }
                }
            }
        }

        snapshots.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(snapshots)
    }

    /// Rollback from specified snapshot to skills_dir
    ///
    /// # Arguments
    /// * `snapshot_name` — snapshot filename, e.g. `curator-2025-08-19T12-34-56.tar.gz`
    /// * `skills_dir` — target restore directory
    pub fn rollback(&self, snapshot_name: &str, skills_dir: &Path) -> Result<()> {
        let snapshot_path = self.backup_dir.join(snapshot_name);
        if !snapshot_path.exists() {
            return Err(BackupError::SnapshotNotFound(snapshot_name.to_string()));
        }

        // Backup current skills (in case rollback fails)
        let temp_backup = self.backup_dir.join("pre-rollback-temp");
        if skills_dir.exists() {
            let file = File::create(&temp_backup)?;
            let encoder = GzEncoder::new(file, flate2::Compression::default());
            let mut builder = Builder::new(encoder);
            builder.append_dir_all("skills", skills_dir)?;
            builder.finish()?;
        }

        // Unpack snapshot (overwrite skills_dir)
        let file = File::open(&snapshot_path)?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        // Clean old content first
        if skills_dir.exists() {
            fs::remove_dir_all(skills_dir)?;
        }
        fs::create_dir_all(skills_dir)?;

        // Unpack (tar internal paths are skills/...)
        for mut entry in archive.entries()? {
            let mut entry_path = match entry {
                Ok(ref e) => match e.path() {
                    Ok(p) => p.to_path_buf(),
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            // Strip "skills/" prefix
            if entry_path.starts_with("skills/") {
                entry_path = entry_path.strip_prefix("skills/").unwrap().to_path_buf();
            } else {
                continue;
            }

            let out_path = skills_dir.join(&entry_path);
            if entry_path.to_string_lossy().ends_with('/') {
                fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).ok();
                }
                if let Ok(ref mut e) = entry { e.unpack(&out_path)?; }
            }
        }

        tracing::info!("Curator rollback complete: {}", snapshot_name);
        Ok(())
    }

    /// Clean old snapshots, keep recent N
    ///
    /// # Arguments
    /// * `keep` — keep recent N snapshots (default 5)
    /// * `dry_run` — if true, only report snapshots to delete, don't actually delete
    pub fn prune_old_snapshots(&self, keep: usize, dry_run: bool) -> Result<Vec<String>> {
        let snapshots = self.list_snapshots()?;

        if snapshots.len() <= keep {
            tracing::debug!("prune: {} snapshots <= keep({}), nothing to prune", snapshots.len(), keep);
            return Ok(vec![]);
        }

        let to_delete: Vec<_> = snapshots.iter().skip(keep).collect();
        let mut deleted = Vec::new();

        for meta in to_delete {
            let filename = format!("curator-{}.tar.gz", meta.timestamp);
            let filepath = self.backup_dir.join(&filename);

            if dry_run {
                tracing::info!("prune dry-run: would delete {}", filename);
            } else if filepath.exists() {
                fs::remove_file(&filepath)?;
                tracing::info!("prune: deleted {}", filename);
            }
            deleted.push(filename);
        }

        Ok(deleted)
    }

    /// Get latest snapshot filename
    pub fn latest_snapshot(&self) -> Result<Option<String>> {
        let snapshots = self.list_snapshots()?;
        Ok(snapshots.first().map(|m| format!("curator-{}.tar.gz", m.timestamp)))
    }

    /// Execute curator run pre-auto snapshot
    ///
    /// Returns snapshot filename; returns None if `skills_dir` doesn't exist (not an error)
    pub fn auto_snapshot(&self, skills_dir: &Path) -> Result<Option<String>> {
        if !skills_dir.exists() {
            return Ok(None);
        }
        let filename = self.snapshot(skills_dir, Some("auto-pre-curator-run"))?;
        // Auto cleanup: keep recent 5
        self.prune_old_snapshots(5, false)?;
        Ok(Some(filename))
    }

    /// Wrapper for snapshot() + auto prune (aligns with Hermes `snapshot_skills(reason)`)
    ///
    /// Difference from `auto_snapshot`:
    /// - Checks curator enabled config (Hermes `_snapshot_skills` logic)
    /// - Checks if skills_dir exists (Hermes logic)
    /// - Calls `snapshot()` to execute backup
    /// - Calls `prune_old_snapshots()` to clean old snapshots
    ///
    /// # Arguments
    /// * `reason` — snapshot reason description, e.g. "pre-curator-run"
    ///
    /// # Returns
    /// * `Some(PathBuf)` — snapshot directory path
    /// * `None` — skip snapshot (disabled/directory doesn't exist/error)
    pub fn snapshot_skills(&self, reason: &str) -> Option<PathBuf> {
        let skills_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("loom")
            .join("skills");

        // 1. Check enabled (Hermes logic)
        // TODO: Read curator.enabled from config.toml
        let enabled = std::env::var("CURATION_ENABLED")
            .map(|v| v != "false")
            .unwrap_or(true);
        if !enabled {
            tracing::debug!("curator backup disabled — skipping snapshot");
            return None;
        }

        // 2. Check if skills_dir exists (Hermes logic)
        if !skills_dir.exists() {
            tracing::debug!("skills dir does not exist — nothing to back up");
            return None;
        }

        // 3. Create backup directory (Hermes logic: mkdir parents=True)
        if fs::create_dir_all(&self.backup_dir).is_err() {
            tracing::debug!("failed to create backup dir {:?}", self.backup_dir);
            return None;
        }

        // 4. Execute snapshot
        let filename = match self.snapshot(&skills_dir, Some(reason)) {
            Ok(name) => name,
            Err(e) => {
                tracing::debug!("snapshot failed: {}", e);
                return None;
            }
        };

        // 5. Prune old snapshots (Hermes logic: _prune_old(keep=get_keep()))
        if self.prune_old_snapshots(5, false).is_err() {
            tracing::debug!("prune_old_snapshots failed");
        }

        tracing::info!("curator snapshot created: {} ({})", filename, reason);

        Some(self.backup_dir.join(filename))
    }
}

impl Default for CuratorBackup {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_skill(name: &str) -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("instructions.md"), format!("# {}", name)).unwrap();
        fs::write(skill_dir.join("config.yaml"), "name: test").unwrap();
        dir.into_path().join(name)
    }

    #[test]
    fn snapshot_and_list() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());

        let skills_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(skills_dir.path()).unwrap();
        fs::write(skills_dir.path().join("test.md"), "test").unwrap();

        let filename = backup.snapshot(skills_dir.path(), Some("test snapshot")).unwrap();
        assert!(filename.starts_with("curator-"));
        assert!(filename.ends_with(".tar.gz"));

        let snapshots = backup.list_snapshots().unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].description.as_deref(), Some("test snapshot"));
    }

    #[test]
    fn rollback() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());

        let skills_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(skills_dir.path()).unwrap();
        fs::write(skills_dir.path().join("test.md"), "original content").unwrap();

        let filename = backup.snapshot(skills_dir.path(), None).unwrap();

        // Modify content
        fs::write(skills_dir.path().join("test.md"), "modified content").unwrap();
        assert_eq!(
            fs::read_to_string(skills_dir.path().join("test.md")).unwrap(),
            "modified content"
        );

        // Rollback
        backup.rollback(&filename, skills_dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(skills_dir.path().join("test.md")).unwrap(),
            "original content"
        );
    }

    #[test]
    fn prune() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());

        // Create 8 snapshots (each snapshot points to same temp dir, but different skill subdir)
        for i in 0..8 {
            let skill_subdir = backup_dir.path().join(format!("skill-{}", i));
            fs::create_dir_all(&skill_subdir).unwrap();
            fs::write(skill_subdir.join("x"), "x").unwrap();
            backup.snapshot(&skill_subdir, Some(&format!("snap-{}", i))).unwrap();
        }

        // Verify files exist
        let snapshots = backup.list_snapshots().unwrap();
        assert_eq!(snapshots.len(), 8, "should have 8 snapshots before prune");

        let deleted = backup.prune_old_snapshots(5, false).unwrap();
        assert_eq!(deleted.len(), 3, "8 - 5 = 3 should be deleted");

        let remaining = backup.list_snapshots().unwrap();
        assert_eq!(remaining.len(), 5, "5 remaining after prune");
    }

    #[test]
    fn latest_snapshot() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());

        let skills_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(skills_dir.path()).unwrap();
        fs::write(skills_dir.path().join("x"), "x").unwrap();

        assert_eq!(backup.latest_snapshot().unwrap(), None);

        backup.snapshot(skills_dir.path(), None).unwrap();
        backup.snapshot(skills_dir.path(), None).unwrap();

        let latest = backup.latest_snapshot().unwrap().unwrap();
        assert!(latest.starts_with("curator-"));
    }

    #[test]
    fn auto_snapshot_skips_nonexistent_dir() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());

        let result = backup.auto_snapshot(Path::new("/nonexistent/path")).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn auto_snapshot_creates_and_prunes() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());

        let skills_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(skills_dir.path()).unwrap();
        fs::write(skills_dir.path().join("x"), "x").unwrap();

        for i in 0..7 {
            let skill_subdir = skills_dir.path().join(format!("skill-{}", i));
            fs::create_dir_all(&skill_subdir).unwrap();
            backup.snapshot(&skill_subdir, None).unwrap();
        }

        // auto_snapshot will auto prune to 5
        backup.auto_snapshot(skills_dir.path()).unwrap();

        let remaining = backup.list_snapshots().unwrap();
        assert_eq!(remaining.len(), 5, "auto_snapshot should prune to 5");
    }
}
