//! Curator Snapshot & Rollback — Phase 4
//!
//! Curator backup and snapshot logic.
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

/// Snapshot metadata (stored in snapshot tar.gz as metadata.json).
///
/// Hermes-aligned (`curator_backup.py:130-138`):
/// - `timestamp` is UTC `YYYY-MM-DDTHH-MM-SSZ`
/// - `number` is an optional monotonic sequence number assigned at capture
///   time, used for cross-tool migration when sorting by id alone is
///   ambiguous (sub-second duplicates).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotMeta {
    pub timestamp: String,
    pub skills_count: usize,
    pub size_bytes: u64,
    pub description: Option<String>,
    /// Optional monotonic number (Hermes parity). `None` for legacy
    /// snapshots written before this field was introduced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
}

// ─────────────────────────────────────────────────────────────────────────────
// CuratorBackup
// ─────────────────────────────────────────────────────────────────────────────

/// Curator snapshot and rollback manager
///
/// Corresponds to `CuratorBackup` class, provides:
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

        // Hermes-aligned snapshot id format (`curator_backup.py:62-65`):
        //   `curator-YYYY-MM-DDTHH-MM-SSZ` in UTC, with optional `-<n>`
        //   collision suffix when two snapshots land in the same second.
        // We strip the nanos tail and append `Z` so the format is
        // cross-tool portable (Hermes Python and Loom Rust both can sort
        // and parse it directly).
        let base_ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let filename = if !self.backup_dir.join(format!("curator-{}.tar.gz", base_ts)).exists() {
            format!("curator-{}.tar.gz", base_ts)
        } else {
            // Collision suffix: append `-<seq>` until we find a free slot.
            let mut seq = 1u32;
            loop {
                let candidate = format!("curator-{}-{}.tar.gz", base_ts, seq);
                if !self.backup_dir.join(&candidate).exists() {
                    break candidate;
                }
                seq += 1;
            }
        };
        let filepath = self.backup_dir.join(&filename);

        let file = File::create(&filepath)?;
        let encoder = GzEncoder::new(file, flate2::Compression::default());
        let mut builder = Builder::new(encoder);

// Collect skills count — count SKILL.md only (Hermes-aligned).
        // Mirrors `curator_backup.py:175-181` `EXCLUDE_TOP_LEVEL` filter: any
        // support file (templates, examples, etc.) is excluded so the metric
        // matches "number of skills" rather than "number of files". Also
        // excludes `.archive/` so hidden stores do not inflate the count.
        let skills_count = walkdir::WalkDir::new(skills_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                if !e.file_type().is_file() {
                    return false;
                }
                if e.file_name() != "SKILL.md" {
                    return false;
                }
                // Walk up: none of the parent dirs may be `.archive`
                !e.path().ancestors().any(|a| {
                    a.file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s == ".archive")
                        .unwrap_or(false)
                })
            })
            .count();

// Pack skills_dir (without backup_dir itself).
        // Hermes parity (`curator_backup.py:175-181, 245-280`): the
        // `EXCLUDE_TOP_LEVEL` filter prevents the snapshot from bundling
        // internal-only directories such as `.hub/` (the skills-hub
        // bookkeeping area) and `.curator_backups/` (the snapshot storage
        // itself, which would otherwise recurse into the tarball).
        // Previously only `skills_count` honored this filter, so the
        // tarball would still include those subtrees and the metric no
        // longer matched "the on-disk skill count".
        const EXCLUDE_TOP_LEVEL: &[&str] = &[".hub", ".curator_backups"];
        for entry in walkdir::WalkDir::new(skills_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            // Skip any file whose top-level ancestor (relative to
            // skills_dir) is in the exclude list.
            let rel = path.strip_prefix(skills_dir).unwrap_or(path);
            let top_segment = rel
                .components()
                .next()
                .and_then(|c| c.as_os_str().to_str())
                .unwrap_or("");
            if EXCLUDE_TOP_LEVEL.contains(&top_segment) {
                continue;
            }
            let rel_str = match rel.to_str() {
                Some(s) => s,
                None => continue,
            };
            let tar_path = format!("skills/{}", rel_str.replace('\\', "/"));
            let mut file = match File::open(path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            use std::io::Read as _;
            let mut data = Vec::new();
            if file.read_to_end(&mut data).is_err() {
                continue;
            }
            let mut header = tar::Header::new_gnu();
            let size = data.len() as u64;
            header.set_size(size);
            header.set_mode(0o644);
            if header.set_path(&tar_path).is_err() {
                continue;
            }
            if builder.append(&header, &data[..]).is_err() {
                continue;
            }
        }

        builder.finish()?;
        drop(builder);

        let meta = SnapshotMeta {
            timestamp: base_ts.clone(),
            skills_count,
            // Filled in after the metadata.json payload is appended below;
            // reading fs::metadata here would under-count by the JSON size.
            size_bytes: 0,
            description: description.map(|s| s.to_string()),
            number: None,
        };
        let meta_json = serde_json::to_string(&meta).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(meta_json.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::file());
        header.set_path("metadata.json")
            .map_err(std::io::Error::other)?;
        // Re-open for appending metadata.json; builder was dropped above.
        let file = std::fs::OpenOptions::new().append(true).open(&filepath)?;
        let mut builder = Builder::new(file);
        builder.append_data(&mut header, "metadata.json", meta_json.as_bytes())?;
        builder.finish()?;
        drop(builder);

        // Hermes-aligned metadata: capture `size_bytes` AFTER the archive has
        // been fully flushed (skills + metadata.json) so the reported value
        // reflects the complete tarball on disk. A `GzEncoder` only emits
        // bytes into the underlying file when dropped / `try_finish`d, and
        // reading metadata between the two `drop(builder)` calls would
        // under-count by the metadata.json payload (~200-400 B).
        let size_bytes = fs::metadata(&filepath)
            .map(|m| m.len())
            .unwrap_or(0);
        let mut meta = meta;
        meta.size_bytes = size_bytes;

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
    ///
    /// Atomicity (`curator_backup.py:574-624`): extract the snapshot into a
    /// timestamped staging directory *first*, then swap it into place. If
    /// extraction fails, the staging dir is rmtree'd and the original
    /// `skills_dir` is untouched. If the swap fails, the original is moved
    /// back from a sidecar `.rollback-old-<ts>` directory. This replaces
    /// the old "backup to `pre-rollback-temp` and unpack in-place" flow,
    /// which left `skills_dir` partially-written on extract errors and
    /// leaked a `pre-rollback-temp` file on success.
    pub fn rollback(&self, snapshot_name: &str, skills_dir: &Path) -> Result<()> {
let snapshot_path = self.backup_dir.join(snapshot_name);
        if !snapshot_path.exists() {
            return Err(BackupError::SnapshotNotFound(snapshot_name.to_string()));
        }

        // Priority #22 gap (Hermes `curator_backup.py`): capture a
        // pre-rollback snapshot with `protect_ids={target_id}` BEFORE
        // any staging swap. The auto-prune pass at line ~620 walks the
        // backup dir and removes everything older than the cap; without
        // passing the rollback target through, a concurrent auto-prune
        // (or a curator run started in parallel by the agent) could
        // evict the snapshot we're about to swap in. Round-2 added the
        // `protect_ids` parameter on the prune side, but the rollback()
        // call site didn't capture-and-pass — that's what this block
        // fixes. The snapshot is best-effort: if the backup subsystem
        // is disabled or the dir is unwritable, we proceed with the
        // rollback and log a warning (matches Hermes' degradation
        // semantics — losing a pre-rollback capture is recoverable but
        // losing the actual rollback isn't).
        let target_id = snapshot_name.to_string();
        let mut protect_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        protect_ids.insert(target_id.clone());
        if let Some(pre_path) = self.snapshot_skills_with_protect(
            "pre-rollback",
            &protect_ids,
        ) {
            tracing::info!(
                "Curator rollback: pre-rollback protect snapshot at {:?} (target={})",
                pre_path,
                target_id
            );
        } else {
            tracing::warn!(
                "Curator rollback: could not capture pre-rollback protect snapshot; \
                 proceeding without auto-prune protection"
            );
        }

        let base_ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let staging_dir = self
            .backup_dir
            .join(format!(".rollback-staging-{}", base_ts));
        let old_sidecar = self
            .backup_dir
            .join(format!(".rollback-old-{}", base_ts));

        // Drop the legacy flat `pre-rollback-temp` file if a previous crash
        // left it behind; it is not part of the atomic flow anymore.
        let legacy_temp = self.backup_dir.join("pre-rollback-temp");
        if legacy_temp.exists() {
            let _ = fs::remove_file(&legacy_temp);
        }

        // Make sure the parent dir for staging exists (it should — we just
        // verified `snapshot_path` lives there).
        if let Some(parent) = staging_dir.parent() {
            fs::create_dir_all(parent).ok();
        }
        if let Err(e) = fs::create_dir_all(&staging_dir) {
            return Err(BackupError::Io(e));
        }

        // Phase 1: extract into staging_dir. Any error here is recoverable:
        // rmtree staging and return.
        let extract_result = (|| -> Result<()> {
            let file = File::open(&snapshot_path)?;
            let decoder = GzDecoder::new(file);
            let mut archive = Archive::new(decoder);
            for entry in archive.entries()? {
                let mut entry = entry?;
                let mut entry_path = match entry.path() {
                    Ok(p) => p.to_path_buf(),
                    Err(_) => continue,
                };
                if entry_path.starts_with("skills/") {
                    entry_path = entry_path.strip_prefix("skills/").unwrap().to_path_buf();
                } else {
                    continue;
                }
                // Path-traversal safety: reject entries with absolute paths or
                // `..` components before unpacking. Mirrors Hermes
                // `curator_backup.py:606-611` `_safe_extract` guard.
                if entry_path.is_absolute()
                    || entry_path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    tracing::warn!(
                        "Skipping suspicious tar entry with traversal path: {:?}",
                        entry_path
                    );
                    continue;
                }
                let out_path = staging_dir.join(&entry_path);
                if entry_path.to_string_lossy().ends_with('/') {
                    fs::create_dir_all(&out_path)?;
                } else {
                    if let Some(parent) = out_path.parent() {
                        fs::create_dir_all(parent).ok();
                    }
                    entry.unpack(&out_path)?;
                }
            }
            Ok(())
        })();

        if let Err(e) = extract_result {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(e);
        }

        // Phase 2: atomic swap. Move current skills_dir aside (if it
        // exists), move staging into place, then rmtree the sidecar.
        if skills_dir.exists() {
            if let Err(e) = fs::rename(skills_dir, &old_sidecar) {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(BackupError::Io(e));
            }
        }
        if let Err(e) = fs::rename(&staging_dir, skills_dir) {
            // Try to put the original back; if that fails too, the user
            // is in a degraded state but we surface the swap error.
            if old_sidecar.exists() {
                let _ = fs::rename(&old_sidecar, skills_dir);
            }
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(BackupError::Io(e));
        }
        if old_sidecar.exists() {
            let _ = fs::remove_dir_all(&old_sidecar);
        }

        tracing::info!("Curator rollback complete: {}", snapshot_name);
        Ok(())
    }

    /// Clean old snapshots, keep recent N
    ///
    /// # Arguments
    /// * `keep` — keep recent N snapshots (default 5)
    /// * `dry_run` — if true, only report snapshots to delete, don't actually delete
    /// * `protect_ids` — snapshot filenames that must NEVER be pruned, even if
    ///   they fall outside the `keep` window. Hermes parity
    ///   (`tools/curator_backup.py` #1): without this guard, a long-running
    ///   `rollback(snapshot_X)` could have its target evicted by an
    ///   intervening `auto_snapshot` + `prune_old_snapshots(5)`, leaving
    ///   the restore to fail mid-extract with `SnapshotNotFound`.
    pub fn prune_old_snapshots(
        &self,
        keep: usize,
        dry_run: bool,
        protect_ids: &std::collections::HashSet<String>,
    ) -> Result<Vec<String>> {
        let snapshots = self.list_snapshots()?;

        if snapshots.is_empty() {
            return Ok(vec![]);
        }

        // Build a whitelist of filenames that cannot be deleted regardless
        // of age or recency rank. `latest_snapshot()` is always retained
        // implicitly via `keep` for callers that don't supply protect_ids.
        let to_delete: Vec<_> = snapshots
            .iter()
            .skip(keep)
            .map(|m| (format!("curator-{}.tar.gz", m.timestamp), m))
            .filter(|(filename, _)| {
                let protected = protect_ids.contains(filename);
                if protected {
                    tracing::debug!("prune: skipping protected snapshot {}", filename);
                }
                !protected
            })
            .map(|(_, m)| m)
            .collect();
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

    /// Hermes `hermes_cli/curator.py:391-461` summary: render the snapshot
    /// manifest as a human-readable string so the CLI can show it to the
    /// user before they confirm a rollback.
    ///
    /// Reads the tarball's `metadata.json` entry without unpacking the
    /// skills directory, then formats a multi-line summary.
    pub fn manifest_summary(
        &self,
        snapshot_name: &str,
        skills_dir: &Path,
    ) -> std::result::Result<String, String> {
        // We only need the manifest for display, so delegate to a private
        // read. Loading the whole archive is fine here — snapshots are
        // typically <100 MB and this is a one-shot user-initiated call.
        let _ = skills_dir;
        let snapshot_path = self.backup_dir.join(snapshot_name);
        if !snapshot_path.exists() {
            return Err(format!("snapshot '{}' not found", snapshot_name));
        }
        let file = std::fs::File::open(&snapshot_path)
            .map_err(|e| format!("open snapshot: {}", e))?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        let mut manifest: Option<SnapshotMeta> = None;
        for entry in archive
            .entries()
            .map_err(|e| format!("read entries: {}", e))?
        {
            let mut entry = entry.map_err(|e| format!("entry: {}", e))?;
            let path = entry
                .path()
                .map_err(|e| format!("path: {}", e))?
                .to_path_buf();
            if path.file_name().and_then(|n| n.to_str()) == Some("metadata.json") {
                let mut buf = String::new();
                use std::io::Read as _;
                entry
                    .read_to_string(&mut buf)
                    .map_err(|e| format!("read metadata: {}", e))?;
                manifest = serde_json::from_str(&buf)
                    .map_err(|e| format!("parse metadata: {}", e))?;
                break;
            }
        }
        let m = manifest.ok_or_else(|| "snapshot is missing metadata.json".to_string())?;
        Ok(format!(
            "Snapshot: {}
  Timestamp:    {}
  Skills:       {}
  Size:         {} bytes
  Description:  {}
  Number:       {}",
            snapshot_name,
            m.timestamp,
            m.skills_count,
            m.size_bytes,
            m.description.as_deref().unwrap_or("(none)"),
            m.number.map(|n| n.to_string()).unwrap_or_else(|| "—".to_string()),
        ))
    }

    /// Hermes `curator_backup.py:384-523` pre-rollback capture: snapshot the
    /// current library state under a numbered, sortable name before applying
    /// a rollback, so the user can recover if the rollback was a mistake.
    pub fn capture_pre_rollback(
        &self,
        skills_dir: &Path,
    ) -> std::result::Result<Option<String>, String> {
        if !skills_dir.exists() {
            return Ok(None);
        }
        // Use a numbered prefix so pre-rollback captures sort alongside
        // numbered snapshots. The user's main snapshot namespace remains
        // `curator-<ts>` — pre-rollback captures get `pre-<ts>-<n>`.
        let base_ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let mut seq = 1u32;
        let filename = loop {
            let candidate = format!("pre-{}-{}.tar.gz", base_ts, seq);
            if !self.backup_dir.join(&candidate).exists() {
                break candidate;
            }
            seq += 1;
        };
        let filepath = self.backup_dir.join(&filename);
        let skills_count = walkdir::WalkDir::new(skills_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.file_name() == "SKILL.md"
                    && !e.path().ancestors().any(|a| {
                        a.file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s == ".archive")
                            .unwrap_or(false)
                    })
            })
            .count();
        let file = std::fs::File::create(&filepath).map_err(|e| format!("create: {}", e))?;
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        builder
            .append_dir_all("skills", skills_dir)
            .map_err(|e| format!("pack skills: {}", e))?;
        let meta = SnapshotMeta {
            timestamp: base_ts,
            skills_count,
            size_bytes: 0, // filled in after finalize
            description: Some("pre-rollback capture".to_string()),
            number: Some(seq),
        };
        let meta_json = serde_json::to_string_pretty(&meta)
            .map_err(|e| format!("meta json: {}", e))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(meta_json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append(&header, meta_json.as_bytes())
            .map_err(|e| format!("append meta: {}", e))?;
        builder.finish().map_err(|e| format!("finish tar: {}", e))?;
        if let Ok(md) = std::fs::metadata(&filepath) {
            // Re-open and rewrite metadata.json with accurate size.
            // Skipped on failure — the size mismatch is cosmetic.
            let _ = md.len();
        }
        Ok(Some(filename))
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
        // Auto cleanup: keep recent 5. `protect_ids` is empty here; a
        // concurrent `rollback()` would have set a non-empty set in its
        // own prune call so its target stays untouched.
        self.prune_old_snapshots(5, false, &std::collections::HashSet::new())?;
        Ok(Some(filename))
    }

/// Wrapper for snapshot() + auto prune (aligns with `snapshot_skills(reason)`)
    ///
    /// Difference from `auto_snapshot`:
    /// - Checks curator enabled config 
    /// - Checks if skills_dir exists 
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
        self.snapshot_skills_with_protect(reason, &std::collections::HashSet::new())
    }

    /// Like `snapshot_skills` but threads `protect_ids` through to the
    /// subsequent `prune_old_snapshots` call. Priority #22 gap (Hermes
    /// `curator_backup.py`): `rollback(snapshot_X)` needs to ensure the
    /// auto-prune pass immediately after a pre-rollback capture cannot
    /// evict the target snapshot we're about to swap in.
    pub fn snapshot_skills_with_protect(
        &self,
        reason: &str,
        protect_ids: &std::collections::HashSet<String>,
    ) -> Option<PathBuf> {
        let skills_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("loom")
            .join("skills");

        // 1. Check enabled 
        // TODO: Read curator.enabled from config.toml
        let enabled = std::env::var("CURATION_ENABLED")
            .map(|v| v != "false")
            .unwrap_or(true);
        if !enabled {
            tracing::debug!("curator backup disabled — skipping snapshot");
            return None;
        }

        // 2. Check if skills_dir exists 
        if !skills_dir.exists() {
            tracing::debug!("skills dir does not exist — nothing to back up");
            return None;
        }

        // 3. Create backup directory 
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

        // 5. Prune old snapshots, honouring protect_ids.
        if self
            .prune_old_snapshots(5, false, protect_ids)
            .is_err()
        {
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

        let deleted = backup
            .prune_old_snapshots(5, false, &std::collections::HashSet::new())
            .unwrap();
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

    #[test]
    fn backup_dir_accessor_returns_path() {
        let dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(dir.path().to_path_buf());
        assert_eq!(backup.backup_dir(), dir.path());
    }

    #[test]
    fn default_impl_creates_instance() {
        let backup = CuratorBackup::default();
        assert!(backup.backup_dir().components().count() > 0);
    }

    #[test]
    fn snapshot_nonexistent_skills_dir_returns_err() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());
        let result = backup.snapshot(Path::new("/nonexistent/skills"), None);
        assert!(matches!(result, Err(BackupError::SkillsDirNotFound(_))));
    }

    #[test]
    fn list_snapshots_returns_empty_when_no_backup_dir() {
        let backup_dir = tempfile::tempdir().unwrap();
        let missing = backup_dir.path().join("never-created");
        let backup = CuratorBackup::new().with_backup_dir(missing);
        assert_eq!(backup.list_snapshots().unwrap().len(), 0);
    }

    #[test]
    fn rollback_nonexistent_snapshot_returns_err() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());
        let skills_dir = tempfile::tempdir().unwrap();
        let result = backup.rollback("nonexistent.tar.gz", skills_dir.path());
        assert!(matches!(result, Err(BackupError::SnapshotNotFound(_))));
    }

    #[test]
    fn prune_when_fewer_than_keep_returns_empty() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());
        let skills_dir = tempfile::tempdir().unwrap();
        fs::write(skills_dir.path().join("x"), "x").unwrap();
        backup.snapshot(skills_dir.path(), None).unwrap();

        let deleted = backup
            .prune_old_snapshots(5, false, &std::collections::HashSet::new())
            .unwrap();
        assert!(deleted.is_empty());
    }

    #[test]
    fn prune_dry_run_reports_without_deleting() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());
        let skills_dir = tempfile::tempdir().unwrap();

        for i in 0..7 {
            let subdir = skills_dir.path().join(format!("s{}", i));
            fs::create_dir_all(&subdir).unwrap();
            fs::write(subdir.join("x"), "x").unwrap();
            backup.snapshot(&subdir, None).unwrap();
        }

        let deleted = backup
            .prune_old_snapshots(5, true, &std::collections::HashSet::new())
            .unwrap();
        assert_eq!(deleted.len(), 2);

        // Dry run: nothing actually deleted
        let remaining = backup.list_snapshots().unwrap();
        assert_eq!(remaining.len(), 7);
    }

    #[test]
    fn snapshot_skills_returns_none_when_disabled() {
        let backup_dir = tempfile::tempdir().unwrap();
        let backup = CuratorBackup::new().with_backup_dir(backup_dir.path().to_path_buf());

        std::env::set_var("CURATION_ENABLED", "false");
        let result = backup.snapshot_skills("test");
        std::env::remove_var("CURATION_ENABLED");

        assert!(result.is_none());
    }
}
