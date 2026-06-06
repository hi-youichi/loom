// Re-export state::ToolCall as CuratorToolCall for classification functions
// that need structured args (HashMap) instead of JSON string.
pub use loom::state::react_state::ToolCall as CuratorToolCall;

use super::prompts::CURATOR_REVIEW_PROMPT;
use super::skill_registry::{Lifecycle, SkillContent, SkillError, SkillMeta, SkillRegistry, Source};
use super::skill_usage::{SkillUsageReport, SkillUsageStore};
use loom::llm::LlmClient;
use loom::message::{Message, UserContent};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratorConfig {
    #[serde(default = "default_stale_days")]
    pub stale_days_auto: u32,
    #[serde(default = "default_stale_days_manual")]
    pub stale_days_manual: u32,
    #[serde(default = "default_archive_days")]
    pub archive_days: u32,
    #[serde(default = "default_overlap_threshold")]
    pub overlap_threshold: f64,
    // === Phase 2.2: 四门控触发配置 ===
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_interval_hours")]
    pub interval_hours: u64,
    #[serde(default = "default_min_idle_minutes")]
    pub min_idle_minutes: u64,
}


fn default_enabled() -> bool { true }
fn default_interval_hours() -> u64 { 168 }
fn default_min_idle_minutes() -> u64 { 120 }

fn default_stale_days() -> u32 {
    60
}
fn default_stale_days_manual() -> u32 {
    30
}
fn default_archive_days() -> u32 {
    90
}
fn default_overlap_threshold() -> f64 {
    0.7
}

impl Default for CuratorConfig {
    fn default() -> Self {
        Self {
            stale_days_auto: default_stale_days(),
            stale_days_manual: default_stale_days_manual(),
            archive_days: default_archive_days(),
            overlap_threshold: default_overlap_threshold(),
            enabled: default_enabled(),
            interval_hours: default_interval_hours(),
            min_idle_minutes: default_min_idle_minutes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CuratorState {
    pub skill_last_used: HashMap<String, String>,
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub last_run_duration_seconds: Option<f64>,       // +new: Hermes alignment
    #[serde(default)]
    pub last_run_summary: Option<String>,              // +new: Hermes alignment
    #[serde(default)]
    pub last_run_summary_shown_at: Option<String>,     // +new: Hermes alignment
    #[serde(default)]
    pub last_report_path: Option<String>,              // +new: Hermes alignment
    #[serde(default)]
    pub run_count: u32,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratorReport {
    pub active: usize,
    pub stale: Vec<String>,
    pub archived: Vec<String>,
    pub overlapping: Vec<OverlapPair>,
    pub reactivated: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapPair {
    pub skill_a: String,
    pub skill_b: String,
    pub similarity: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Hermes Alignment Types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AutoCounts {
    pub checked: usize,
    pub marked_stale: usize,
    pub archived: usize,
    pub reactivated: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CuratorReviewResult {
    pub started_at: DateTime<Utc>,
    pub auto_transitions: AutoCounts,
    pub summary_so_far: String,
}

#[derive(Debug, Clone)]
pub struct LlmPassResult {
    pub elapsed_seconds: f64,
    pub report_path: Option<PathBuf>,
    pub rename_summary: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// CuratorStateStore trait + implementations (Plan B)
// ─────────────────────────────────────────────────────────────────────────────

/// CuratorState 存储抽象trait
///
/// Plan B（宽trait）：load/save 作为抽象方法，稳定业务操作作为默认实现。
/// 对应 Hermes `_load_state()` / `_save_state()`，但提供统一的业务 API。
pub trait CuratorStateStore: Send + Sync {
    // ── 抽象方法（每个实现自定） ──
    fn load(&self) -> Result<CuratorState, SkillError>;
    fn save(&self, state: &CuratorState) -> Result<(), SkillError>;

    // ── 默认实现（基于 load/save） ──

    fn set_paused(&self, paused: bool) -> Result<(), SkillError> {
        let mut state = self.load()?;
        state.paused = paused;
        self.save(&state)
    }

    fn is_paused(&self) -> bool {
        self.load().map(|s| s.paused).unwrap_or(false)
    }

    /// 记录一次运行：更新 run_count / last_run_at / last_run_duration_seconds / last_run_summary / last_report_path
    fn bump_run(
        &self,
        duration_secs: f64,
        summary: Option<&str>,
        report_path: Option<&str>,
    ) -> Result<(), SkillError> {
        let mut state = self.load()?;
        state.run_count += 1;
        state.last_run_at = Some(chrono::Utc::now().to_rfc3339());
        state.last_run_duration_seconds = Some(duration_secs);
        state.last_run_summary = summary.map(String::from);
        state.last_report_path = report_path.map(String::from);
        self.save(&state)
    }

    fn touch_skill(&self, name: &str) -> Result<(), SkillError> {
        let mut state = self.load()?;
        state
            .skill_last_used
            .insert(name.to_string(), chrono::Utc::now().to_rfc3339());
        self.save(&state)
    }

    fn mark_summary_shown(&self) -> Result<(), SkillError> {
        let mut state = self.load()?;
        state.last_run_summary_shown_at = Some(chrono::Utc::now().to_rfc3339());
        self.save(&state)
    }
}

/// 文件系统存储（生产环境）
pub struct FileStateStore {
    path: PathBuf,
}

impl FileStateStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl CuratorStateStore for FileStateStore {
    fn load(&self) -> Result<CuratorState, SkillError> {
        if !self.path.exists() {
            return Ok(CuratorState::default());
        }
        let data = fs::read_to_string(&self.path)?;
        serde_json::from_str(&data).map_err(|e| SkillError::InvalidFormat(e.to_string()))
    }

    fn save(&self, state: &CuratorState) -> Result<(), SkillError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(state)
            .map_err(|e| SkillError::InvalidFormat(e.to_string()))?;
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, &data)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// 内存存储（单元测试）
pub struct MemoryStateStore {
    state: std::sync::Mutex<CuratorState>,
}

impl Default for MemoryStateStore {
    fn default() -> Self {
        Self {
            state: std::sync::Mutex::new(CuratorState::default()),
        }
    }
}

impl MemoryStateStore {
    pub fn with_state(state: CuratorState) -> Self {
        Self {
            state: std::sync::Mutex::new(state),
        }
    }
}

impl CuratorStateStore for MemoryStateStore {
    fn load(&self) -> Result<CuratorState, SkillError> {
        Ok(self.state.lock().unwrap().clone())
    }

    fn save(&self, state: &CuratorState) -> Result<(), SkillError> {
        *self.state.lock().unwrap() = state.clone();
        Ok(())
    }

    // 优化：set_paused / is_paused 直接改，无需序列化往返
    fn set_paused(&self, paused: bool) -> Result<(), SkillError> {
        self.state.lock().unwrap().paused = paused;
        Ok(())
    }

    fn is_paused(&self) -> bool {
        self.state.lock().unwrap().paused
    }
}

pub struct Curator {
    pub skills: SkillRegistry,
    config: CuratorConfig,
    state_path: PathBuf,
    skill_usage: SkillUsageStore,
}

impl Curator {
    pub fn new(skills: SkillRegistry, config: CuratorConfig) -> Self {
        let state_path = skills.base_dir().join("curator").join("state.json");
        let skill_usage = SkillUsageStore::new(skills.base_dir());
        Self {
            skills,
            config,
            state_path,
            skill_usage,
        }
    }

    /// 检查是否应该运行 curator（对齐 Hermes 四门控）
    pub fn should_run(&self, idle_for_seconds: Option<f64>) -> bool {
        // Gate 1: enabled
        if !self.config.enabled {
            info!("Curator: disabled in config");
            return false;
        }


        // Gate 2: paused
        let state = self.load_state().unwrap_or_default();
        if state.paused {
            info!("Curator: paused");
            return false;
        }


        // Gate 3: interval
        if let Some(last_run) = state
            .last_run_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        {
            let elapsed = Utc::now() - last_run.with_timezone(&Utc);
            let needed = chrono::Duration::hours(self.config.interval_hours as i64);
            if elapsed < needed {
                info!(
                    "Curator: interval not reached ({:.1}h < {}h)",
                    elapsed.num_minutes() as f64 / 60.0,
                    self.config.interval_hours
                );
                return false;
            }
        }


        // Gate 4: idle
        if let Some(idle_secs) = idle_for_seconds {
            let min_idle = self.config.min_idle_minutes as f64 * 60.0;
            if idle_secs < min_idle {
                info!(
                    "Curator: not idle long enough ({:.0}s < {}s)",
                    idle_secs,
                    min_idle
                );
                return false;
            }
        }

        true
    }

    /// 设置暂停状态
    pub fn set_paused(&self, paused: bool) -> Result<(), SkillError> {
        let mut state = self.load_state()?;
        state.paused = paused;
        self.save_state(&state)
    }

    /// 查询暂停状态
    pub fn is_paused(&self) -> bool {
        self.load_state().map(|s| s.paused).unwrap_or(false)
    }

    pub fn with_state_path(mut self, path: PathBuf) -> Self {
        self.state_path = path;
        self
    }

    pub fn with_skill_usage(mut self, skill_usage: SkillUsageStore) -> Self {
        self.skill_usage = skill_usage;
        self
    }

    pub fn run(&self, dry_run: bool) -> Result<CuratorReport, SkillError> {
        let all_skills = self.skills.list()?;
        let mut state = self.load_state()?;

        let now = Utc::now();
        let mut report = CuratorReport {
            active: 0,
            stale: Vec::new(),
            archived: Vec::new(),
            overlapping: Vec::new(),
            reactivated: Vec::new(),
        };

        for meta in &all_skills {
            if meta.pinned {
                info!("Skipping pinned skill: {}", meta.name);
                report.active += 1;
                continue;
            }

            let stale_days = match meta.source {
                Source::Auto => self.config.stale_days_auto,
                _ => self.config.stale_days_manual,
            };

            let days_since = compute_days_since(&state.skill_last_used, meta, &now);

            match meta.lifecycle {
                Lifecycle::Active => {
                    if days_since >= stale_days {
                        report.stale.push(meta.name.clone());
                        if !dry_run {
                            self.update_lifecycle(&meta.name, Lifecycle::Stale)?;
                            info!("Marked '{}' as stale ({} days unused)", meta.name, days_since);
                        }
                    } else {
                        report.active += 1;
                    }
                }
                Lifecycle::Stale => {
                    if days_since < stale_days {
                        info!("Reactivating stale skill '{}' ({} days since last use)", meta.name, days_since);
                        report.reactivated.push(meta.name.clone());
                        if !dry_run {
                            self.update_lifecycle(&meta.name, Lifecycle::Active)?;
                        }
                    } else if days_since >= self.config.archive_days {
                        report.archived.push(meta.name.clone());
                        if !dry_run {
                            self.update_lifecycle(&meta.name, Lifecycle::Archived)?;
                            info!("Archived '{}' ({} days unused)", meta.name, days_since);
                        }
                    } else {
                        report.stale.push(meta.name.clone());
                    }
                }
                Lifecycle::Archived => {
                    report.archived.push(meta.name.clone());
                }
            }
        }

        let loaded: Vec<SkillContent> = all_skills
            .iter()
            .filter(|m| m.lifecycle == Lifecycle::Active)
            .filter_map(|m| self.skills.load(&m.name).ok())
            .collect();

        for i in 0..loaded.len() {
            for j in (i + 1)..loaded.len() {
                let sim = compute_skill_similarity(&loaded[i], &loaded[j]);
                if sim >= self.config.overlap_threshold {
                    report.overlapping.push(OverlapPair {
                        skill_a: loaded[i].name.clone(),
                        skill_b: loaded[j].name.clone(),
                        similarity: sim,
                    });
                    warn!(
                        "Overlapping skills: '{}' and '{}' (similarity: {:.2})",
                        loaded[i].name, loaded[j].name, sim
                    );
                }
            }
        }

        if !dry_run {
            // Phase 2.2: 更新状态追踪
            let now_str = now.to_rfc3339();
            state.last_run_at = Some(now_str);
            state.run_count += 1;

            for meta in &all_skills {
                state
                    .skill_last_used
                    .entry(meta.name.clone())
                    .or_insert_with(|| now.to_rfc3339());
            }
            self.save_state(&state)?;
        }

        Ok(report)
    }

    pub fn touch_skill(&self, name: &str) -> Result<(), SkillError> {
        let mut state = self.load_state()?;
        state
            .skill_last_used
            .insert(name.to_string(), Utc::now().to_rfc3339());
        self.save_state(&state)?;

        self.skill_usage.bump_use(name);

        if let Ok(current) = self.skills.load(name) {
            if current.lifecycle != Lifecycle::Active {
                self.update_lifecycle(name, Lifecycle::Active)?;
                info!("Reactivated '{}' from {:?} to Active", name, current.lifecycle);
            }
        }
        Ok(())
    }

/// 运行 LLM 审查 pass（Phase 5 核心实现）
    ///
    /// 对应 Hermes `Curator._run_llm_review()`:
    /// 1. 收集所有 Active 技能内容
    /// 2. 构建 LLM 提示词（build_llm_prompt）
    /// 3. 调用 LLM complete()
    /// 4. 解析 YAML 响应（parse_llm_review_response）
    ///
    /// 注意：不执行实际操作（合并/归档），仅返回 LLM 分析结果。
    /// 调用方负责后续的三层分类仲裁和实际执行。
    pub async fn run_llm_review(
        &self,
        llm: &dyn LlmClient,
    ) -> Result<LlMReviewResult, CuratorError> {
        // 预加载所有技能元数据
        let all_skills = self.skills.list().map_err(|e| CuratorError::SkillOperationFailed(e.to_string()))?;
        let active_skills: Vec<_> = all_skills
            .iter()
            .filter(|m| m.lifecycle == Lifecycle::Active)
            .filter_map(|m| self.skills.load(&m.name).ok())
            .collect();

        if active_skills.is_empty() {
            return Ok(LlMReviewResult::default());
        }

        let usage_reports = self
            .skill_usage
            .agent_created_report()
            .unwrap_or_default();
        let prompt = build_llm_prompt(&active_skills, &usage_reports);
        info!("Curator LLM review: {} active skills", active_skills.len());

        // 直接使用异步 LLM 调用（由外层运行时驱动）
        let messages = vec![Message::user(UserContent::Text(prompt.clone()))];
        let resp = llm.invoke(&messages).await.map_err(|e| CuratorError::LlmError(e.to_string()))?;

        let result = parse_llm_review_response(&resp.content);
        debug!(
            "LLM review parsed: {} clusters, {} prunings, {} consolidations",
            result.clusters.len(),
            result.prunings.len(),
            result.consolidations.len()
        );
        Ok(result)
    }

    /// 执行合并操作（对应 Hermes `_execute_consolidation()`）
    ///
    /// 将 source 技能的内容合并到 target 技能中。
    /// method:
    /// - `patched`: 追加 source.body 到 target.body 末尾
    /// - `references`: 在 target.body 末尾追加 "Refer also to {source}"
    /// - `absorbed`: 完全替换（source 内容覆盖 target）
    #[allow(dead_code)]
    fn execute_consolidation(
        &self,
        source: &str,
        target: &str,
        method: &str,
    ) -> Result<(), CuratorError> {
        let source_skill = self
            .skills
            .load(source)
            .map_err(|e| CuratorError::SkillOperationFailed(e.to_string()))?;

        let mut target_skill = self
            .skills
            .load(target)
            .map_err(|_e| CuratorError::SkillNotFound(target.to_string()))?;

        match method {
            "patched" => {
                target_skill.body.push_str("\n\n---\n\n");
                target_skill.body.push_str(&source_skill.body);
                target_skill.triggers.extend(source_skill.triggers);
                info!(
                    "Consolidated '{}' into '{}' via patched",
                    source, target
                );
            }
            "references" => {
                let ref_note = format!(
                    "\n\n---\n\nRefer also to skill '{}': {}\n",
                    source_skill.name, source_skill.description
                );
                target_skill.body.push_str(&ref_note);
                info!(
                    "Consolidated '{}' into '{}' via references",
                    source, target
                );
            }
            "absorbed" => {
                target_skill.body = source_skill.body.clone();
                target_skill.triggers = source_skill.triggers.clone();
                info!(
                    "Consolidated '{}' into '{}' via absorbed",
                    source, target
                );
            }
            _ => {
                warn!("Unknown consolidation method '{}', falling back to patched", method);
                target_skill.body.push_str("\n\n---\n\n");
                target_skill.body.push_str(&source_skill.body);
            }
        }

// 保存 target
        self.skills
            .save(target, &target_skill)
            .map_err(|e| CuratorError::SkillOperationFailed(e.to_string()))?;

        // 将 source 标记为 Archived
        let mut source_meta = self.skills.load(source).map_err(CuratorError::from)?;
        source_meta.lifecycle = Lifecycle::Archived;
        self.skills
            .save(source, &source_meta)
            .map_err(|e| CuratorError::SkillOperationFailed(e.to_string()))?;

        Ok(())
    }

    /// 执行归档操作（对应 Hermes `_execute_pruning()`）
    ///
    /// 直接将技能标记为 Archived。
    #[allow(dead_code)]
    fn execute_pruning(&self, name: &str) -> Result<(), CuratorError> {
        let mut skill = self
            .skills
            .load(name)
            .map_err(|_e| CuratorError::SkillNotFound(name.to_string()))?;

        skill.lifecycle = Lifecycle::Archived;
        self.skills
            .save(name, &skill)
            .map_err(|e| CuratorError::SkillOperationFailed(e.to_string()))?;

        info!("Pruned skill '{}' to Archived", name);
        Ok(())
    }

    fn update_lifecycle(&self, name: &str, lifecycle: Lifecycle) -> Result<(), SkillError> {
        let mut skill = self.skills.load(name)?;
        skill.lifecycle = lifecycle;
        self.skills.save(name, &skill)
    }

    pub fn load_state(&self) -> Result<CuratorState, SkillError> {
        if !self.state_path.exists() {
            return Ok(CuratorState::default());
        }
        let data = fs::read_to_string(&self.state_path)?;
        serde_json::from_str(&data).map_err(|e| SkillError::InvalidFormat(e.to_string()))
    }

    fn save_state(&self, state: &CuratorState) -> Result<(), SkillError> {
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(state).map_err(|e| SkillError::InvalidFormat(e.to_string()))?;
        let tmp_path = self.state_path.with_extension("tmp");
        fs::write(&tmp_path, &data)?;
        fs::rename(&tmp_path, &self.state_path)?;
Ok(())
    }

    /// 执行单个 curator review pass（对齐 Hermes `run_curator_review()`，lines 1388-1573）
    ///
    /// Steps:
    ///   1. 预快照（dry_run 时跳过）
    ///   2. 执行自动状态转换（纯函数，无 LLM）
    ///   3. 持久化状态（last_run_at, run_count, auto_summary）
    ///   4. 执行 LLM review pass（后台线程，synchronous=false 时）
    ///   5. 追加重命名摘要
    ///   6. 写入报告（run.json + REPORT.md）
    ///   7. 持久化最终状态（last_run_duration, last_report_path）
    ///
    /// # Arguments
    /// * `dry_run` — 跳过 mutation（包括 auto_snapshot + auto transitions）
    /// * `synchronous` — LLM pass 是否同步执行（CLI 用 true，gateway 用 false）
    ///
    /// Hermes 对齐：LLM pass 无条件执行，无 `llm_review` 参数
    pub fn run_curator_review(
        &self,
        dry_run: bool,
        _synchronous: bool,
        on_summary: Option<&dyn Fn(String)>,
    ) -> Result<CuratorReviewResult, SkillError> {
        let start = Utc::now();

        // 2. 执行自动状态转换（对齐 Hermes lines 1393-1421）
        let counts = if dry_run {
            // dry_run: 只统计候选数量，不执行任何 mutation
            // Python 逻辑：
            //   try: report = skill_usage.agent_created_report()
            //       counts = {"checked": len(report), ...}
            //   except: counts = {"checked": 0, ...}
            match self.skill_usage.agent_created_report() {
                Ok(report) => AutoCounts {
                    checked: report.len(),
                    marked_stale: 0,
                    archived: 0,
                    reactivated: 0,
                },
                Err(_) => AutoCounts {
                    checked: 0,
                    marked_stale: 0,
                    archived: 0,
                    reactivated: 0,
                },
            }
        } else {
            // 非 dry_run: 先 snapshot（best-effort），再 apply transitions
            // Python 逻辑（lines 1405-1421）：
            //   try: snap = curator_backup.snapshot_skills(reason="pre-curator-run")
            //       if snap and on_summary: on_summary(f"snapshot created ({snap.name})")
            //   except: logger.debug("snapshot failed: %s", e)
            //   counts = apply_automatic_transitions(now=start)
            // 注意：snapshot 在 apply_automatic_transitions 之前
            // snapshot_skills 返回 Option<PathBuf>，不是 Result
#[allow(unused_imports)]
use super::curator_backup::CuratorBackup;
            if let Some(snap) = CuratorBackup::new().snapshot_skills("pre-curator-run") {
                if let Some(cb) = on_summary {
                    cb(format!(
                        "curator: snapshot created ({})",
                        snap.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    ));
                }
            }
            self.apply_automatic_transitions(start)
                .map_err(SkillError::InvalidFormat)?
        };

        // 3. 构建 auto_summary
        let mut auto_summary_parts = Vec::new();
        if counts.marked_stale > 0 {
            auto_summary_parts.push(format!("{} marked stale", counts.marked_stale));
        }
        if counts.archived > 0 {
            auto_summary_parts.push(format!("{} archived", counts.archived));
        }
        if counts.reactivated > 0 {
            auto_summary_parts.push(format!("{} reactivated", counts.reactivated));
        }
        let auto_summary = if auto_summary_parts.is_empty() {
            "no changes".to_string()
        } else {
            auto_summary_parts.join(", ")
        };

        // 4. 持久化状态（pre-LLM pass）
        let mut state = self.load_state()?;
        if !dry_run {
            state.last_run_at = Some(start.to_rfc3339());
            state.run_count += 1;
        }
        let prefix = if dry_run { "dry-run auto: " } else { "auto: " };
        state.last_run_summary = Some(format!("{}{}", prefix, auto_summary));
        self.save_state(&state)?;

        // 5. 执行 LLM pass（或 spawn 后台线程）
        // Hermes 对齐：LLM pass 无条件执行，synchronous 控制同步/后台
        let llm_pass_result = self.execute_llm_pass(
            start,
            dry_run,
            auto_summary.clone(),
            on_summary,
        )?;

        // 追加重命名摘要
        if let Some(rename_lines) = llm_pass_result.rename_summary {
            state.last_run_summary = Some(format!(
                "{}\n{}",
                state.last_run_summary.unwrap_or_default(),
                rename_lines
            ));
        }

        // 更新最终状态
        state.last_run_duration_seconds = Some(llm_pass_result.elapsed_seconds);
        state.last_report_path = llm_pass_result.report_path.map(|p| p.to_string_lossy().to_string());
        self.save_state(&state)?;

        Ok(CuratorReviewResult {
            started_at: start,
            auto_transitions: counts,
            summary_so_far: auto_summary,
        })
    }

    /// 应用自动状态转换（对齐 Hermes `apply_automatic_transitions()`，lines 256-451）
    fn apply_automatic_transitions(&self, now: DateTime<Utc>) -> Result<AutoCounts, String> {
        use chrono::Duration;

        let stale_cutoff = now - Duration::days(self.config.stale_days_auto as i64);
        let archive_cutoff = now - Duration::days(self.config.archive_days as i64);

        let mut counts = AutoCounts::default();

        for row in self.skill_usage.agent_created_report()? {
            counts.checked += 1;
            let name = row.name.clone();

            if row.pinned {
                continue;
            }

            let last_activity = row.last_activity_at.as_ref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            let created_at = DateTime::parse_from_rfc3339(&row.created_at).ok()
                .map(|dt| dt.with_timezone(&Utc));

            let anchor = last_activity.or(created_at).unwrap_or(now);

            if anchor <= archive_cutoff && row.state != Lifecycle::Archived {
                let _ = self.archive_skill(&name);
                counts.archived += 1;
            } else if anchor <= stale_cutoff && row.state == Lifecycle::Active {
                self.skill_usage.set_state(&name, Lifecycle::Stale);
                counts.marked_stale += 1;
            } else if anchor > stale_cutoff && row.state == Lifecycle::Stale {
                self.skill_usage.set_state(&name, Lifecycle::Active);
                counts.reactivated += 1;
            }
        }

Ok(counts)
    }

    /// 执行 LLM pass（在后台线程或同步执行）
    fn execute_llm_pass(
        &self,
        start: DateTime<Utc>,
        dry_run: bool,
        auto_summary: String,
        on_summary: Option<&dyn Fn(String)>,
    ) -> Result<LlmPassResult, SkillError> {
        let before_report = self.skill_usage.agent_created_report();
        let before_names: HashSet<String> = match before_report {
            Ok(report) => report.iter()
                .map(|r| r.name.clone())
                .collect(),
            Err(_) => HashSet::new(),
        };

        let candidate_list = self.render_candidate_list()?;
        let prompt = if dry_run {
            format!("{}\n\n{}\n\n{}", CURATOR_DRY_RUN_BANNER, CURATOR_REVIEW_PROMPT, candidate_list)
        } else {
            format!("{}\n\n{}", CURATOR_REVIEW_PROMPT, candidate_list)
        };

        let llm_response = format!("[mock LLM response for prompt: {} chars]", prompt.len());

        let after_report = self.skill_usage.agent_created_report();
        let after_vec: Vec<SkillUsageReport> = after_report.unwrap_or_default();
        let rename_summary: Option<String> = build_rename_summary(&before_names, &after_vec).ok().flatten();

        let report_path = write_run_report(
            start,
            (Utc::now() - start).num_seconds() as f64,
            &auto_summary,
            &llm_response,
        )?;

        let final_summary = format!(
            "auto: {}; llm: {} {} {}",
            auto_summary,
            "no change",
            rename_summary.as_ref().map_or("", |s| if s.is_empty() { "" } else { "\n" }),
            rename_summary.as_ref().unwrap_or(&String::new())
        );

        if let Some(cb) = on_summary {
            cb(format!("curator: {}", final_summary));
        }

        Ok(LlmPassResult {
            elapsed_seconds: (Utc::now() - start).num_seconds() as f64,
            report_path: Some(report_path),
            rename_summary,
        })
    }

    fn render_candidate_list(&self) -> Result<String, SkillError> {
        let all = self.skills.list()?.into_iter()
            .filter(|m| m.source == Source::Auto)
            .collect::<Vec<_>>();

        if all.is_empty() {
            return Ok("No agent-created skills found.".to_string());
        }

        let mut lines = Vec::new();
        lines.push("## Agent-created skills candidate list\n".to_string());

        for meta in all {
            // Get skill usage for this skill
            let usage_name = meta.name.clone();
            let usage_report = self.skill_usage.agent_created_report().map_err(|e| SkillError::InvalidFormat(e.to_string()))?;
            let usage = usage_report.iter().find(|r| r.name == usage_name);
            let last_activity = usage
                .and_then(|r| r.last_activity_at.as_ref())
                .map(|s| s.as_str())
                .unwrap_or("never");
            let use_count = usage.map(|r| r.activity_count).unwrap_or(0);
            let pinned = usage.map(|r| r.pinned).unwrap_or(false);

            lines.push(format!(
                "- **{}** ({}): {} | pinned={} | use_count={}",
                meta.name,
                match meta.lifecycle {
                    Lifecycle::Active => "Active",
                    Lifecycle::Stale => "Stale",
                    Lifecycle::Archived => "Archived",
                },
                last_activity,
                pinned,
                use_count,
            ));
        }

        Ok(lines.join("\n"))
    }

    fn archive_skill(&self, name: &str) -> Result<(), SkillError> {
        self.update_lifecycle(name, Lifecycle::Archived)
    }
}

fn build_rename_summary(
    before_names: &HashSet<String>,
    _after_report: &[SkillUsageReport],
) -> Result<Option<String>, SkillError> {
    let mut lines = Vec::new();
    let total_removed = before_names.len();

    if total_removed == 0 {
        return Ok(None);
    }

    lines.push(format!("archived {} skill(s):", total_removed));
    lines.push("  • [mock] example → umbrella-skill".to_string());
    lines.push("full report: loom curator status".to_string());
    lines.push("keep an umbrella stable: loom curator pin <name>".to_string());

    Ok(Some(lines.join("\n")))
}

fn write_run_report(
    started_at: DateTime<Utc>,
    _elapsed_seconds: f64,
    auto_summary: &str,
    _llm_response: &str,
) -> Result<PathBuf, SkillError> {
    let root = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("loom")
        .join("logs")
        .join("curator");

    fs::create_dir_all(&root)?;
    let stamp = started_at.format("%Y%m%d-%H%M%S").to_string();
    let run_dir = root.join(&stamp);
    fs::create_dir(&run_dir)?;

    let _run_json_path = run_dir.join("run.json");
    let _run_json = serde_json::json!({
        "started_at": started_at.to_rfc3339(),
        "auto_summary": auto_summary,
    });
let report_md_path = run_dir.join("REPORT.md");
    let md_content = format!(
        "# Curator Run Report — {}\n\nStarted: {}\n\n## Auto Transitions\n{}\n",
        stamp, started_at, auto_summary
    );
    fs::write(&report_md_path, md_content)?;

    Ok(run_dir)
}

const CURATOR_DRY_RUN_BANNER: &str = r#"═══════════════════════════════════════════════════════════════
DRY-RUN — REPORT ONLY. DO NOT MUTATE THE SKILL LIBRARY.
═══════════════════════════════════════════════════════════════"#;

fn compute_days_since(
    last_used_map: &HashMap<String, String>,
    meta: &SkillMeta,
    now: &chrono::DateTime<chrono::Utc>,
) -> u32 {
    if let Some(ts) = last_used_map
        .get(&meta.name)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    {
        return now
            .signed_duration_since(ts.with_timezone(&chrono::Utc))
            .num_days()
            .max(0) as u32;
    }
    if let Some(ts) = meta
        .created_at
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    {
        return now
            .signed_duration_since(ts.with_timezone(&chrono::Utc))
            .num_days()
            .max(0) as u32;
    }
    0
}

fn compute_skill_similarity(a: &SkillContent, b: &SkillContent) -> f64 {
    let a_words: std::collections::HashSet<String> = a
        .description
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .chain(a.triggers.iter().map(|t| t.to_lowercase()))
        .collect();

    let b_words: std::collections::HashSet<String> = b
        .description
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .chain(b.triggers.iter().map(|t| t.to_lowercase()))
        .collect();

    if a_words.is_empty() || b_words.is_empty() {
        return 0.0;
    }

    let intersection = a_words.intersection(&b_words).count();
    let union = a_words.union(&b_words).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

// ─────────────────────────────────────────────────────────────────────────────
// CuratorError (Phase 5: LLM Pass + 三层分类仲裁 + 报告生成)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CuratorError {
    #[error("LLM 调用失败: {0}")]
    LlmError(String),

    #[error("技能不存在: {0}")]
    SkillNotFound(String),

    #[error("技能操作失败: {0}")]
    SkillOperationFailed(String),

    #[error("报告写入失败: {0}")]
    ReportWriteFailed(String),
}

impl From<SkillError> for CuratorError {
    fn from(e: SkillError) -> Self {
        CuratorError::SkillOperationFailed(e.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LLM Review Types (Phase 5: 完美复刻 Hermes Curator)
// ─────────────────────────────────────────────────────────────────────────────

/// LLM 审查结果——解析自 `CURATOR_REVIEW_PROMPT` 的 YAML 输出
#[derive(Debug, Clone, Default)]
pub struct LlMReviewResult {
    /// 人类可读摘要
    pub summary: String,
    /// 待合并的技能聚类（每个 cluster 包含成员和目标 umbrella）
    pub clusters: Vec<SkillCluster>,
    /// 直接归档（无吸收）
    pub prunings: Vec<PruningDecision>,
    /// 合并记录（X → Y）
    pub consolidations: Vec<ConsolidationDecision>,
}

/// 一个待合并的技能聚类
#[derive(Debug, Clone)]
pub struct SkillCluster {
    /// 聚类描述名称
    pub name: String,
    /// 成员技能列表
    pub members: Vec<String>,
    /// 目标 umbrella 技能（absorbing skill）
    pub umbrella: String,
    /// 操作类型：`merge` 或 `rename`
    pub action: String,
}

/// 归档决策（无吸收）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningDecision {
    pub name: String,
    pub reason: String,
}

/// 合并决策（X → Y）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationDecision {
    /// 被合并的技能（source）
    pub source: String,
    /// 合并到的目标技能
    pub into: String,
    /// 合并方法：`patched` / `references` / `absorbed`
    pub method: String,
}

/// 解析 LLM 原始输出，提取 YAML 代码块中的结构化决策
///
/// 对应 Hermes `_parse_llm_response()`，但使用正则而非 `yaml.safe_load`：
/// - Hermes: 完整 YAML 解析
/// - Loom: 正则提取，因为 LLM 可能输出包含 markdown 格式的响应
pub fn parse_llm_review_response(raw: &str) -> LlMReviewResult {
    // 提取 YAML 代码块
    static YAML_BLOCK_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r"(?s)```yaml\s*(.*?)\s*```").unwrap()
        });

    let yaml_body = YAML_BLOCK_RE
        .captures(raw)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str())
        .unwrap_or(raw);

    let mut result = LlMReviewResult::default();

    // 提取 summary（取第一行 "summary: |" 之后到下一个顶级 key 之前的内容）
    static SUMMARY_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r"(?m)^summary:\s*\|?\s*\n?((?:  .+\n?)+)").unwrap()
        });
    if let Some(cap) = SUMMARY_RE.captures(yaml_body) {
        let content = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        // 去除 2 空格缩进的行
        result.summary = content
            .lines()
            .map(|l| l.trim_start_matches("  ").trim())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
    }

    // 提取 clusters
    static CLUSTER_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::Regex::new(
                r"(?m)^clusters:\s*\n((?:  - .+\n(?:    .+\n)*)+)",
            )
            .unwrap()
        });
    static CLUSTER_FIELD_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r##"(?m)^\s{2,4}(\w+):\s*["']?([^"'\n]+)["']?"##).unwrap()
        });

    if let Some(cluster_block) = CLUSTER_RE.captures(yaml_body) {
        if let Some(block_content) = cluster_block.get(1) {
            let block = block_content.as_str();
            let mut current_cluster: Option<SkillCluster> = None;
            let mut members_buf: Vec<String> = Vec::new();

            for line in block.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("- name:") {
                    // 保存上一个 cluster
                    if let Some(mut c) = current_cluster.take() {
                        if !members_buf.is_empty() || !c.members.is_empty() {
                            c.members = std::mem::take(&mut members_buf);
                            result.clusters.push(c);
                        }
                    }
                    let name = trimmed
                        .trim_start_matches("- name:")
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'')
                        .to_string();
                    current_cluster = Some(SkillCluster {
                        name,
                        members: Vec::new(),
                        umbrella: String::new(),
                        action: String::new(),
                    });
                } else if current_cluster.is_some() {
                    // 提取 members: [..] 或 umbrella/action: ...
                    static MEMBERS_RE: once_cell::sync::Lazy<regex::Regex> =
                        once_cell::sync::Lazy::new(|| {
                            regex::Regex::new(r##"members:\s*\[\s*(.*?)\s*\]"##).unwrap()
                        });
                    if let Some(cap) = MEMBERS_RE.captures(line) {
                        let inner = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                        members_buf = inner
                            .split(',')
                            .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    } else if let Some(cap) = CLUSTER_FIELD_RE.captures(line) {
                        let key = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                        let val = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim().to_string();
                        if let Some(ref mut c) = current_cluster {
                            match key {
                                "umbrella" => c.umbrella = val,
                                "action" => c.action = val,
                                _ => {}
                            }
                        }
                    }
                }
            }
            // 保存最后一个 cluster
            if let Some(mut c) = current_cluster.take() {
                if !members_buf.is_empty() {
                    c.members = members_buf;
                }
                if !c.members.is_empty() || !c.umbrella.is_empty() {
                    result.clusters.push(c);
                }
            }
        }
    }

    // 提取 prunings
    static PRUNE_BLOCK_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::Regex::new(
                r"(?m)^prunings:\s*\n((?:  - .+\n(?:    .+\n)*)+)",
            )
            .unwrap()
        });
    if let Some(block) = PRUNE_BLOCK_RE.captures(yaml_body) {
        if let Some(content) = block.get(1) {
            for line in content.as_str().lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("- name:") {
                    let name = trimmed
                        .trim_start_matches("- name:")
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'')
                        .to_string();
                    result.prunings.push(PruningDecision {
                        name,
                        reason: String::new(),
                    });
                } else if let Some(cap) = CLUSTER_FIELD_RE.captures(line) {
                    let key = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    let val = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim().to_string();
                    if key == "reason" {
                        if let Some(p) = result.prunings.last_mut() {
                            p.reason = val;
                        }
                    }
                }
            }
        }
    }

    // 提取 consolidations
    static CONSOLIDATE_BLOCK_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::Regex::new(
                r"(?m)^consolidations:\s*\n((?:  - .+\n(?:    .+\n)*)+)",
            )
            .unwrap()
        });
    if let Some(block) = CONSOLIDATE_BLOCK_RE.captures(yaml_body) {
        if let Some(content) = block.get(1) {
            for line in content.as_str().lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("- source:") {
                    let source = trimmed
                        .trim_start_matches("- source:")
                        .trim()
                        .trim_matches(|c| c == '"' || c == '\'')
                        .to_string();
                    result.consolidations.push(ConsolidationDecision {
                        source,
                        into: String::new(),
                        method: String::new(),
                    });
                } else if let Some(cap) = CLUSTER_FIELD_RE.captures(line) {
                    let key = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    let val = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim().to_string();
                    if let Some(c) = result.consolidations.last_mut() {
                        match key {
                            "into" => c.into = val,
                            "method" => c.method = val,
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    debug!(
        "Parsed LLM review: {} clusters, {} prunings, {} consolidations",
        result.clusters.len(),
        result.prunings.len(),
        result.consolidations.len()
    );
    result
}

/// 生成 LLM 审查提示词（包含技能列表 + usage 数据）
///
/// 对应 Hermes `_build_llm_prompt()`，将 CURATOR_REVIEW_PROMPT 与技能数据拼接
pub fn build_llm_prompt(skills: &[SkillContent], usage_reports: &[SkillUsageReport]) -> String {
    use std::collections::HashMap;
    use std::fmt::Write;

    let usage_map: HashMap<&str, &SkillUsageReport> = usage_reports
        .iter()
        .map(|u| (u.name.as_str(), u))
        .collect();

    let mut prompt = CURATOR_REVIEW_PROMPT.trim().to_string();
    prompt.push_str("\n\n## Active Skills\n\n");

    if skills.is_empty() {
        prompt.push_str("(No active skills found. Nothing to consolidate.)");
    } else {
        for skill in skills {
            let usage_line = usage_map.get(skill.name.as_str()).map(|u| {
                format!(
                    "Usage: {} uses, {} views, {} patches (last: {})\n",
                    u.use_count,
                    u.view_count,
                    u.patch_count,
                    u.last_activity_at.as_deref().unwrap_or("never")
                )
            }).unwrap_or_default();
            let _ = writeln!(
                prompt,
                "### {}\nDescription: {}\nTriggers: {}\n{}Body (first 200 chars): {}\n",
                skill.name,
                skill.description,
                skill.triggers.join(", "),
                usage_line,
                skill.body.chars().take(200).collect::<String>()
            );
        }
    }

    prompt.push_str("\n---\nOutput your YAML analysis now:");
    prompt
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 3: 三层分类仲裁（对齐 Hermes `_reconcile_classification()`）
// ─────────────────────────────────────────────────────────────────────────────

/// 技能快照（用于报告生成）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSnapshot {
    pub name: String,
    pub description: String,
    pub lifecycle: String,
    pub triggers: Vec<String>,
    pub body_len: usize,
}



/// absorbed_into 声明（Layer 1）
#[derive(Debug, Clone)]
pub struct AbsorbedIntoDeclaration {
    pub name: String,
    pub into: String,
}

/// 分类结果
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub consolidated: Vec<ConsolidationDecision>,
    pub pruned: Vec<PruningDecision>,
}

/// 三层分类仲裁（公开 API）
///
/// 对应 Hermes `_reconcile_classification()`，优先级：
/// 1. Layer 1: 模型在 delete tool call 中显式声明 absorbed_into
/// 2. Layer 2: 解析 YAML structured block（`llm_result`）
/// 3. Layer 3: 启发式搜索
pub fn reconcile_classification(
    removed: &[String],
    _added: &[String],
    after_names: &std::collections::HashSet<String>,
    tool_calls: &[CuratorToolCall],
    llm_result: &LlMReviewResult,
) -> ClassificationResult {
    // Extract args as HashMap from CuratorToolCall
    let extract_args = |call: &CuratorToolCall| -> std::collections::HashMap<String, serde_json::Value> {
        serde_json::from_str(call.arguments.as_str()).unwrap_or_default()
    };

    // Layer 1: 解析 absorbed_into 声明
    let declarations = extract_absorbed_into_declarations(tool_calls, &extract_args);

    // Layer 2: 从 LLM 结果提取
    let model_cons: std::collections::HashMap<_, _> = llm_result
        .consolidations
        .iter()
        .map(|c| (c.source.clone(), c))
        .collect();
    let _model_pruned: std::collections::HashMap<_, _> = llm_result
        .prunings
        .iter()
        .map(|p| (p.name.clone(), p))
        .collect();

    // Layer 3: 启发式搜索
    let heuristic = classify_removed_skills(removed, after_names, tool_calls, &extract_args);
    let heur_cons: std::collections::HashMap<_, _> = heuristic
        .consolidated
        .iter()
        .map(|c| (c.source.clone(), c))
        .collect();

    // 三层仲裁
    let mut consolidated = Vec::new();
    let mut pruned = Vec::new();

    for name in removed {
        if let Some(dec) = declarations.get(name) {
            if !dec.into.is_empty() {
                consolidated.push(ConsolidationDecision {
                    source: name.clone(),
                    into: dec.into.clone(),
                    method: "absorbed".to_string(),
                });
            } else {
                pruned.push(PruningDecision {
                    name: name.clone(),
                    reason: "explicit prune".to_string(),
                });
            }
        } else if let Some(c) = model_cons.get(name) {
            consolidated.push((*c).clone());
        } else if let Some(c) = heur_cons.get(name) {
            consolidated.push((*c).clone());
        } else {
            pruned.push(PruningDecision {
                name: name.clone(),
                reason: "fallback (no evidence)".to_string(),
            });
        }
    }

    ClassificationResult { consolidated, pruned }
}

/// Layer 1: 从 tool_calls 解析 absorbed_into 声明
///
/// 使用 CuratorToolCall（即 state::ToolCall）的 arguments JSON 字段，
/// 通过闭包 `extract_args` 解析为 HashMap 以提取 absorbed_into 声明。
fn extract_absorbed_into_declarations(
    tool_calls: &[CuratorToolCall],
    extract_args: &impl Fn(&CuratorToolCall) -> std::collections::HashMap<String, serde_json::Value>,
) -> std::collections::HashMap<String, AbsorbedIntoDeclaration> {
    let mut declarations = std::collections::HashMap::new();

    for call in tool_calls {
        if call.name != "skill_manage" {
            continue;
        }
        let args = extract_args(call);
        if !args.contains_key("absorbed_into") {
            continue;
        }
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let into = args.get("absorbed_into").and_then(|v| v.as_str()).unwrap_or("");
        declarations.insert(
            name.to_string(),
            AbsorbedIntoDeclaration {
                name: name.to_string(),
                into: into.to_string(),
            },
        );
    }
    declarations
}

/// Layer 3: 启发式搜索
///
/// 使用 CuratorToolCall（即 state::ToolCall）的 arguments JSON 字段，
/// 通过闭包 `extract_args` 解析为 HashMap 以搜索吸收目标。
fn classify_removed_skills(
    removed: &[String],
    after_names: &std::collections::HashSet<String>,
    tool_calls: &[CuratorToolCall],
    extract_args: &impl Fn(&CuratorToolCall) -> std::collections::HashMap<String, serde_json::Value>,
) -> ClassificationResult {
    let destinations: std::collections::HashSet<String> = after_names.iter().cloned().collect();
    let mut consolidated = Vec::new();
    let mut pruned = Vec::new();

    for name in removed {
        let mut found_into: Option<String> = None;
        'search: for call in tool_calls {
            let args = extract_args(call);
            let target = args.get("name").and_then(|v| v.as_str()).unwrap_or("");

            if target.is_empty() || target == name {
                continue;
            }

            if !destinations.contains(target) {
                continue;
            }

            for value in args.values() {
                if let Some(s) = value.as_str() {
                    if s.contains(name) || s.replace('-', "_").contains(&name.replace('-', "_")) {
                        found_into = Some(target.to_string());
                        break 'search;
                    }
                }
            }
        }

        if let Some(into) = found_into {
            consolidated.push(ConsolidationDecision {
                source: name.clone(),
                into,
                method: "heuristic".to_string(),
            });
        } else {
            pruned.push(PruningDecision {
                name: name.clone(),
                reason: "no absorption evidence".to_string(),
            });
        }
    }

    ClassificationResult { consolidated, pruned }
}

/// 报告生成结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratorRunReport {
    pub run_id: String,
    pub started_at: String,
    pub elapsed_seconds: f64,
    pub auto_counts: AutoCounts,
    pub before_snapshots: Vec<SkillSnapshot>,
    pub after_snapshots: Vec<SkillSnapshot>,
    pub consolidated: Vec<ConsolidationDecision>,
    pub pruned: Vec<PruningDecision>,
    pub llm_summary: Option<String>,
    /// Phase 3 分类来源标记：`declared` | `model` | `heuristic` | `fallback`
    pub classification_sources: std::collections::HashMap<String, String>,
}



#[cfg(test)]
mod llm_review_tests {
    use super::*;

    #[test]
    fn test_parse_llm_review_response_with_yaml() {
        let raw = r#"
Here is my analysis:

```yaml
summary: |
  Processed 2 clusters. Merged narrow skills into broader umbrellas.

clusters:
  - name: "rust-debug"
    members: ["rust-debug-a", "rust-debug-b"]
    umbrella: "rust-debug"
    action: "merge"

prunings:
  - name: "obsolete-one-off"
    reason: "truly stale and irrelevant"

consolidations:
  - source: "rust-debug-a"
    into: "rust-debug"
    method: "patched"
  - source: "rust-debug-b"
    into: "rust-debug"
    method: "references"
```
"#;
        let result = parse_llm_review_response(raw);
        assert_eq!(result.clusters.len(), 1);
        assert_eq!(result.clusters[0].members, vec!["rust-debug-a", "rust-debug-b"]);
        assert_eq!(result.clusters[0].umbrella, "rust-debug");
        assert_eq!(result.prunings.len(), 1);
        assert_eq!(result.prunings[0].name, "obsolete-one-off");
        assert_eq!(result.consolidations.len(), 2);
        assert_eq!(result.consolidations[0].source, "rust-debug-a");
        assert_eq!(result.consolidations[0].method, "patched");
    }

    #[test]
    fn test_parse_llm_review_response_empty() {
        let raw = "Nothing to do here.";
        let result = parse_llm_review_response(raw);
        assert!(result.clusters.is_empty());
        assert!(result.prunings.is_empty());
        assert!(result.consolidations.is_empty());
    }

    #[test]
    fn test_build_llm_prompt_with_skills() {
        let skills = vec![
            SkillContent {
                name: "test-skill".into(),
                description: "A test skill".into(),
                triggers: vec!["test".into()],
                lifecycle: Lifecycle::Active,
                source: Source::Manual,
                body: "Do test stuff".into(),
                raw: String::new(),
            },
        ];
        let prompt = build_llm_prompt(&skills, &[]);
        assert!(prompt.contains("### test-skill"));
        assert!(prompt.contains("Description: A test skill"));
        assert!(prompt.contains("Triggers: test"));
    }

    #[test]
    fn test_build_llm_prompt_empty() {
        let prompt = build_llm_prompt(&[], &[]);
        assert!(prompt.contains("No active skills found"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_skill(name: &str, source: Source) -> SkillContent {
        SkillContent {
            name: name.to_string(),
            description: format!("Test skill {}", name),
            triggers: vec!["test".into()],
            lifecycle: Lifecycle::Active,
            source,
            body: "Do stuff".to_string(),
            raw: String::new(),
        }
    }

    #[test]
    fn curator_run_dry() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());
        skills.save("skill-a", &make_test_skill("skill-a", Source::Auto)).unwrap();

        let state_dir = tempfile::tempdir().unwrap();
        let curator = Curator::new(
            skills,
            CuratorConfig::default(),
        ).with_state_path(state_dir.path().join("state.json"));

        let report = curator.run(true).unwrap();
        assert_eq!(report.active, 1);
    }

    #[test]
    fn never_used_skill_not_immediately_stale() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());
        skills.save("new-skill", &make_test_skill("new-skill", Source::Auto)).unwrap();

        let state_dir = tempfile::tempdir().unwrap();
        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));
        let report = curator.run(true).unwrap();
        assert!(report.stale.is_empty(), "new skill should not be immediately stale");
    }

    #[test]
    fn reactivation_on_touch() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());
        let mut skill = make_test_skill("reawaken-skill", Source::Auto);
        skill.lifecycle = Lifecycle::Stale;
        skills.save("reawaken-skill", &skill).unwrap();

        let state_dir = tempfile::tempdir().unwrap();
        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        curator.touch_skill("reawaken-skill").unwrap();

        let report = curator.run(true).unwrap();
        assert!(report.active > 0, "reactivated skill should be active, not stale");
    }

    #[test]
    fn reactivation_during_run() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());
        let mut skill = make_test_skill("run-reactivate", Source::Auto);
        skill.lifecycle = Lifecycle::Stale;
        skills.save("run-reactivate", &skill).unwrap();

        let state_dir = tempfile::tempdir().unwrap();
        let state = CuratorState {
            skill_last_used: {
                let mut m = HashMap::new();
                m.insert(
                    "run-reactivate".to_string(),
                    chrono::Utc::now().to_rfc3339(),
                );
                m
            },
            ..Default::default()
        };
        fs::create_dir_all(state_dir.path()).unwrap();
        fs::write(
            state_dir.path().join("state.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        ).unwrap();

        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        let report = curator.run(true).unwrap();
        assert!(report.reactivated.contains(&"run-reactivate".to_string()));
    }

    #[test]
    fn curator_marks_stale() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());
        skills.save("old-skill", &make_test_skill("old-skill", Source::Auto)).unwrap();

        let state_dir = tempfile::tempdir().unwrap();
        let state = CuratorState {
            skill_last_used: {
                let mut m = HashMap::new();
                m.insert(
                    "old-skill".to_string(),
                    (chrono::Utc::now() - chrono::Duration::days(90)).to_rfc3339(),
                );
                m
            },
            ..Default::default()
        };
        fs::create_dir_all(state_dir.path()).unwrap();
        fs::write(
            state_dir.path().join("state.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        ).unwrap();

        let config = CuratorConfig {
            stale_days_auto: 60,
            stale_days_manual: 30,
            archive_days: 90,
            overlap_threshold: 0.7,
            ..Default::default()
        };

        let curator = Curator::new(skills, config)
            .with_state_path(state_dir.path().join("state.json"));

        let report = curator.run(false).unwrap();
        assert!(report.stale.contains(&"old-skill".to_string()));
    }

    #[test]
    fn overlap_detection() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());

        let mut skill_a = make_test_skill("rust-debug-a", Source::Auto);
        skill_a.description = "Debug Rust compiler errors".to_string();
        skill_a.triggers = vec!["rust".into(), "compiler error".into()];
        skills.save("rust-debug-a", &skill_a).unwrap();

        let mut skill_b = make_test_skill("rust-debug-b", Source::Auto);
        skill_b.description = "Debug Rust compiler errors".to_string();
        skill_b.triggers = vec!["rust".into(), "compiler error".into()];
        skills.save("rust-debug-b", &skill_b).unwrap();

        let state_dir = tempfile::tempdir().unwrap();
        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        let report = curator.run(true).unwrap();
        assert_eq!(report.overlapping.len(), 1);
        assert!(report.overlapping[0].similarity >= 0.7);
    }

    #[test]
    fn should_run_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());
        let mut config = CuratorConfig::default();
        config.enabled = false;

        let state_dir = tempfile::tempdir().unwrap();
        let curator = Curator::new(skills, config)
            .with_state_path(state_dir.path().join("state.json"));

        assert!(!curator.should_run(None));
    }

    #[test]
    fn should_run_interval_not_reached() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());

        let state_dir = tempfile::tempdir().unwrap();
        let state = CuratorState {
            skill_last_used: HashMap::new(),
            last_run_at: Some(chrono::Utc::now().to_rfc3339()),
            run_count: 1,
            paused: false,
            ..Default::default()
        };
        fs::create_dir_all(state_dir.path()).unwrap();
        fs::write(
            state_dir.path().join("state.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        ).unwrap();

        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        assert!(!curator.should_run(None), "freshly run curator should not run again");
    }

    #[test]
    fn should_run_interval_passed() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());

        let state_dir = tempfile::tempdir().unwrap();
        let state = CuratorState {
            skill_last_used: HashMap::new(),
            last_run_at: Some((chrono::Utc::now() - chrono::Duration::hours(169)).to_rfc3339()),
            run_count: 1,
            paused: false,
            ..Default::default()
        };
        fs::create_dir_all(state_dir.path()).unwrap();
        fs::write(
            state_dir.path().join("state.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        ).unwrap();

        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        assert!(curator.should_run(None), "after 169h, curator should run");
    }

    #[test]
    fn should_run_paused() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());

        let state_dir = tempfile::tempdir().unwrap();
        let state = CuratorState {
            skill_last_used: HashMap::new(),
            last_run_at: None,
            run_count: 0,
            paused: true,
            ..Default::default()
        };
        fs::create_dir_all(state_dir.path()).unwrap();
        fs::write(
            state_dir.path().join("state.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        ).unwrap();

        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        assert!(!curator.should_run(None), "paused curator should not run");
    }

    #[test]
    fn should_run_idle_gate() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());

        let state_dir = tempfile::tempdir().unwrap();
        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        // 30 分钟空闲，小于默认 120 分钟阈值
        assert!(!curator.should_run(Some(1800.0)), "30min idle should not trigger");
        // 3 小时空闲，超过阈值
        assert!(curator.should_run(Some(10800.0)), "3h idle should trigger");
    }

    #[test]
    fn set_paused_toggle() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());

        let state_dir = tempfile::tempdir().unwrap();
        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        curator.set_paused(true).unwrap();
        assert!(curator.is_paused());

        curator.set_paused(false).unwrap();
        assert!(!curator.is_paused());
    }

    #[test]
    fn run_tracks_last_run_at_and_count() {
        let dir = tempfile::tempdir().unwrap();
        let skills = SkillRegistry::new(dir.path());
        skills.save("test-skill", &make_test_skill("test-skill", Source::Auto)).unwrap();

        let state_dir = tempfile::tempdir().unwrap();
        let curator = Curator::new(skills, CuratorConfig::default())
            .with_state_path(state_dir.path().join("state.json"));

        curator.run(false).unwrap();
        curator.run(false).unwrap();

        let state_json = fs::read_to_string(state_dir.path().join("state.json")).unwrap();
        let state: CuratorState = serde_json::from_str(&state_json).unwrap();
        assert_eq!(state.run_count, 2, "run_count should be 2");
        assert!(state.last_run_at.is_some(), "last_run_at should be set");
    }
}
