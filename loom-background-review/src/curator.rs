// Re-export ToolCall from loom-llm for classification functions
// that need structured args (HashMap) instead of JSON string.
pub use loom_llm::tool::ToolCall as CuratorToolCall;

use super::prompts::CURATOR_REVIEW_PROMPT;
use super::skill_registry::{Lifecycle, SkillContent, SkillError, SkillMeta, SkillRegistry, Source};
use super::skill_usage::{SkillUsageReport, SkillUsageStore};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
    // === Phase 2.2: Four gate trigger config ===
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
    pub last_run_duration_seconds: Option<f64>,
    #[serde(default)]
    pub last_run_summary: Option<String>,
    #[serde(default)]
    pub last_run_summary_shown_at: Option<String>,
    #[serde(default)]
    pub last_report_path: Option<String>,
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
// CuratorStateStore trait + implementations
// ─────────────────────────────────────────────────────────────────────────────

/// CuratorState storage abstraction trait
pub trait CuratorStateStore: Send + Sync {
    // Abstract methods (each implementation defines)
    fn load(&self) -> Result<CuratorState, SkillError>;
    fn save(&self, state: &CuratorState) -> Result<(), SkillError>;

    // Default implementations (based on load/save)

    fn set_paused(&self, paused: bool) -> Result<(), SkillError> {
        let mut state = self.load()?;
        state.paused = paused;
        self.save(&state)
    }

    fn is_paused(&self) -> bool {
        self.load().map(|s| s.paused).unwrap_or(false)
    }

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

/// File system storage (production)
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

/// Memory storage (unit tests)
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

    pub fn with_state_path(mut self, path: PathBuf) -> Self {
        self.state_path = path;
        self
    }

    pub fn with_skill_usage(mut self, skill_usage: SkillUsageStore) -> Self {
        self.skill_usage = skill_usage;
        self
    }

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

    pub fn set_paused(&self, paused: bool) -> Result<(), SkillError> {
        let mut state = self.load_state()?;
        state.paused = paused;
        self.save_state(&state)
    }

    pub fn is_paused(&self) -> bool {
        self.load_state().map(|s| s.paused).unwrap_or(false)
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
}

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
// LLM Review Types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LlMReviewResult {
    pub summary: String,
    pub clusters: Vec<SkillCluster>,
    pub prunings: Vec<PruningDecision>,
    pub consolidations: Vec<ConsolidationDecision>,
}

#[derive(Debug, Clone)]
pub struct SkillCluster {
    pub name: String,
    pub members: Vec<String>,
    pub umbrella: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningDecision {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationDecision {
    pub source: String,
    pub into: String,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSnapshot {
    pub name: String,
    pub description: String,
    pub lifecycle: String,
    pub triggers: Vec<String>,
    pub body_len: usize,
}

#[derive(Debug, Clone)]
pub struct AbsorbedIntoDeclaration {
    pub name: String,
    pub into: String,
}

#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub consolidated: Vec<ConsolidationDecision>,
    pub pruned: Vec<PruningDecision>,
}

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
    pub classification_sources: std::collections::HashMap<String, String>,
}

/// Parse LLM YAML response (curator review output)
pub fn parse_llm_review_response(raw: &str) -> LlMReviewResult {
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

    static SUMMARY_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r"(?m)^summary:\s*\|?\s*\n?((?:  .+\n?)+)").unwrap()
        });
    if let Some(cap) = SUMMARY_RE.captures(yaml_body) {
        let content = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        result.summary = content
            .lines()
            .map(|l| l.trim_start_matches("  ").trim())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
    }

    // Simplified parsing - in a full implementation, this would extract
    // clusters, prunings, and consolidations from the YAML

    debug!(
        "Parsed LLM review: {} clusters, {} prunings, {} consolidations",
        result.clusters.len(),
        result.prunings.len(),
        result.consolidations.len()
    );
    result
}

/// Build LLM prompt for curator review
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

/// Three-layer classification arbitration
pub fn reconcile_classification(
    removed: &[String],
    _added: &[String],
    after_names: &std::collections::HashSet<String>,
    tool_calls: &[CuratorToolCall],
    llm_result: &LlMReviewResult,
) -> ClassificationResult {
    let extract_args = |call: &CuratorToolCall| -> std::collections::HashMap<String, serde_json::Value> {
        serde_json::from_str(call.arguments.as_str()).unwrap_or_default()
    };

    let declarations = extract_absorbed_into_declarations(tool_calls, &extract_args);

    let model_cons: std::collections::HashMap<_, _> = llm_result
        .consolidations
        .iter()
        .map(|c| (c.source.clone(), c))
        .collect();

    let heuristic = classify_removed_skills(removed, after_names, tool_calls, &extract_args);
    let heur_cons: std::collections::HashMap<_, _> = heuristic
        .consolidated
        .iter()
        .map(|c| (c.source.clone(), c))
        .collect();

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
        assert!(result.summary.contains("2 clusters"));
    }

    #[test]
    fn test_parse_llm_review_response_empty() {
        let raw = "Nothing to do here.";
        let result = parse_llm_review_response(raw);
        assert!(result.clusters.is_empty());
        assert!(result.prunings.is_empty());
        assert!(result.consolidations.is_empty());
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
}
