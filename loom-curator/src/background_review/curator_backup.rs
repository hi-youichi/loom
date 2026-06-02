//! Curator Snapshot & Rollback — Phase 4
//!
//! 参考 Hermes `curator_backup.py` (693 行) 的核心安全能力：
//! - `snapshot()` — 执行前自动备份 skills 目录为 tar.gz
//! - `rollback()` — 从快照回滚
//! - `list_snapshots()` — 列出可用快照
//! - `prune_old_snapshots()` — 清理旧快照（保留最近 N 个）
//!
//! 设计原则：
//! - 同步操作（不依赖 async），因为 Curator 本身是同步的
//! - 快照目录：`~/.loom/backups/` 或 `state_path` 的同级目录
//! - 命名格式：`curator-{timestamp}.tar.gz`

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
    #[error("技能目录不存在: {0}")]
    SkillsDirNotFound(PathBuf),
    #[error("快照不存在: {0}")]
    SnapshotNotFound(String),
    #[error("快照目录初始化失败: {0}")]
    BackupDirInitFailed(PathBuf),
}

pub type Result<T> = std::result::Result<T, BackupError>;

// ─────────────────────────────────────────────────────────────────────────────
// Snapshot metadata
// ─────────────────────────────────────────────────────────────────────────────

/// 快照元数据（存储在快照 tar.gz 内的 metadata.json）
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

/// Curator 快照与回滚管理器
///
/// 对应 Hermes `CuratorBackup` 类，提供：
/// - 自动快照（在 curator 修改 skills 前）
/// - 手动/自动回滚
/// - 快照列表与清理
#[derive(Debug, Clone)]
pub struct CuratorBackup {
    /// 快照存储目录（默认为 `~/.loom/backups/`）
    backup_dir: PathBuf,
}

impl CuratorBackup {
    /// 使用默认备份目录（`~/.loom/backups/`）
    pub fn new() -> Self {
        let backup_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("loom")
            .join("backups");
        Self { backup_dir }
    }

    /// 使用自定义备份目录
    pub fn with_backup_dir(mut self, backup_dir: PathBuf) -> Self {
        self.backup_dir = backup_dir;
        self
    }

    /// 快照目录
    pub fn backup_dir(&self) -> &Path {
        &self.backup_dir
    }

    /// 创建技能目录的快照（tar.gz）
    ///
    /// 返回快照文件名（不含路径），如 `curator-2025-08-19T12-34-56.tar.gz`
    ///
    /// # Arguments
    /// * `skills_dir` — `.loom/skills/` 目录
    /// * `description` — 可选的快照描述
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

        // 收集技能数量
        let skills_count = walkdir::WalkDir::new(skills_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count();

        // 打包 skills_dir（不包含 backup_dir 自身）
        builder.append_dir_all("skills", skills_dir)?;

        // 写入 metadata.json（使用 append_data，需要 tar 0.4.13+ Header::set_path）
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

    /// 列出所有快照（按时间倒序）
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

    /// 从指定快照回滚到 skills_dir
    ///
    /// # Arguments
    /// * `snapshot_name` — 快照文件名，如 `curator-2025-08-19T12-34-56.tar.gz`
    /// * `skills_dir` — 目标恢复目录
    pub fn rollback(&self, snapshot_name: &str, skills_dir: &Path) -> Result<()> {
        let snapshot_path = self.backup_dir.join(snapshot_name);
        if !snapshot_path.exists() {
            return Err(BackupError::SnapshotNotFound(snapshot_name.to_string()));
        }

        // 备份当前 skills（万一回滚出问题）
        let temp_backup = self.backup_dir.join("pre-rollback-temp");
        if skills_dir.exists() {
            let file = File::create(&temp_backup)?;
            let encoder = GzEncoder::new(file, flate2::Compression::default());
            let mut builder = Builder::new(encoder);
            builder.append_dir_all("skills", skills_dir)?;
            builder.finish()?;
        }

        // 解压快照（覆盖 skills_dir）
        let file = File::open(&snapshot_path)?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        // 先清理旧内容
        if skills_dir.exists() {
            fs::remove_dir_all(skills_dir)?;
        }
        fs::create_dir_all(skills_dir)?;

        // 解压（tar 内路径为 skills/...）
        for mut entry in archive.entries()? {
            let mut entry_path = match entry {
                Ok(ref e) => match e.path() {
                    Ok(p) => p.to_path_buf(),
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            // 去掉 "skills/" 前缀
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

    /// 清理旧快照，保留最近 N 个
    ///
    /// # Arguments
    /// * `keep` — 保留最近 N 个快照（默认 5）
    /// * `dry_run` — 如果 true，只报告要删除的快照，不实际删除
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

    /// 获取最新的快照文件名
    pub fn latest_snapshot(&self) -> Result<Option<String>> {
        let snapshots = self.list_snapshots()?;
        Ok(snapshots.first().map(|m| format!("curator-{}.tar.gz", m.timestamp)))
    }

    /// 执行 curator run 前的自动快照
    ///
    /// 返回快照文件名；如果 `skills_dir` 不存在则返回 None（不算错误）
    pub fn auto_snapshot(&self, skills_dir: &Path) -> Result<Option<String>> {
        if !skills_dir.exists() {
            return Ok(None);
        }
        let filename = self.snapshot(skills_dir, Some("auto-pre-curator-run"))?;
        // 自动清理：保留最近 5 个
        self.prune_old_snapshots(5, false)?;
        Ok(Some(filename))
    }

    /// 包装 snapshot() + 自动 prune（对齐 Hermes `snapshot_skills(reason)`）
    ///
    /// 与 `auto_snapshot` 的区别：
    /// - 检查 curator enabled 配置（Hermes `_snapshot_skills` 逻辑）
    /// - 检查 skills_dir 是否存在（Hermes 逻辑）
    /// - 调用 `snapshot()` 执行备份
    /// - 调用 `prune_old_snapshots()` 清理旧快照
    ///
    /// # Arguments
    /// * `reason` — 快照原因描述，如 "pre-curator-run"
    ///
    /// # Returns
    /// * `Some(PathBuf)` — 快照目录路径
    /// * `None` — 跳过快照（禁用/目录不存在/错误）
    pub fn snapshot_skills(&self, reason: &str) -> Option<PathBuf> {
        let skills_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("loom")
            .join("skills");

        // 1. 检查 enabled（Hermes 逻辑）
        // TODO: 从 config.toml 读取 curator.enabled
        let enabled = std::env::var("CURATION_ENABLED")
            .map(|v| v != "false")
            .unwrap_or(true);
        if !enabled {
            tracing::debug!("curator backup disabled — skipping snapshot");
            return None;
        }

        // 2. 检查 skills_dir 存在（Hermes 逻辑）
        if !skills_dir.exists() {
            tracing::debug!("skills dir does not exist — nothing to back up");
            return None;
        }

        // 3. 创建备份目录（Hermes 逻辑：mkdir parents=True）
        if fs::create_dir_all(&self.backup_dir).is_err() {
            tracing::debug!("failed to create backup dir {:?}", self.backup_dir);
            return None;
        }

        // 4. 执行快照
        let filename = match self.snapshot(&skills_dir, Some(reason)) {
            Ok(name) => name,
            Err(e) => {
                tracing::debug!("snapshot failed: {}", e);
                return None;
            }
        };

        // 5. Prune 旧快照（Hermes 逻辑：_prune_old(keep=get_keep())）
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

        // 修改内容
        fs::write(skills_dir.path().join("test.md"), "modified content").unwrap();
        assert_eq!(
            fs::read_to_string(skills_dir.path().join("test.md")).unwrap(),
            "modified content"
        );

        // 回滚
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

        // 创建 8 个快照（每个快照指向同一个临时目录，但会被不同 skill 子目录）
        for i in 0..8 {
            let skill_subdir = backup_dir.path().join(format!("skill-{}", i));
            fs::create_dir_all(&skill_subdir).unwrap();
            fs::write(skill_subdir.join("x"), "x").unwrap();
            backup.snapshot(&skill_subdir, Some(&format!("snap-{}", i))).unwrap();
        }

        // 验证文件存在
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

        // auto_snapshot 会自动 prune 到 5
        backup.auto_snapshot(skills_dir.path()).unwrap();

        let remaining = backup.list_snapshots().unwrap();
        assert_eq!(remaining.len(), 5, "auto_snapshot should prune to 5");
    }
}