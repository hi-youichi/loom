use super::skill_registry::{Lifecycle, SkillContent, SkillError, SkillRegistry, Source};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};
use chrono::Utc;

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
}

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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratorReport {
    pub active: usize,
    pub stale: Vec<String>,
    pub archived: Vec<String>,
    pub overlapping: Vec<OverlapPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapPair {
    pub skill_a: String,
    pub skill_b: String,
    pub similarity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CuratorState {
    skill_last_used: HashMap<String, String>,
}

pub struct Curator {
    skills: SkillRegistry,
    config: CuratorConfig,
    state_path: PathBuf,
}

impl Curator {
    pub fn new(skills: SkillRegistry, config: CuratorConfig) -> Self {
        let state_path = skills.base_dir().join("curator").join("state.json");
        Self {
            skills,
            config,
            state_path,
        }
    }

    pub fn with_state_path(mut self, path: PathBuf) -> Self {
        self.state_path = path;
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
        };

        for meta in &all_skills {
            let stale_days = match meta.source {
                Source::Auto => self.config.stale_days_auto,
                _ => self.config.stale_days_manual,
            };
            let _stale_threshold = now - chrono::Duration::days(stale_days as i64);
            let _archive_threshold = now - chrono::Duration::days(self.config.archive_days as i64);

            let last_used = state
                .skill_last_used
                .get(&meta.name)
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());

            let days_since = last_used.map_or(u32::MAX, |lu| {
                (now - lu.with_timezone(&chrono::Utc))
                    .num_days()
                    .max(0) as u32
            });

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
                    if days_since >= self.config.archive_days {
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
        self.save_state(&state)
    }

    fn update_lifecycle(&self, name: &str, lifecycle: Lifecycle) -> Result<(), SkillError> {
        let mut skill = self.skills.load(name)?;
        skill.lifecycle = lifecycle;
        self.skills.save(name, &skill)
    }

    fn load_state(&self) -> Result<CuratorState, SkillError> {
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
        fs::write(&self.state_path, data)?;
        Ok(())
    }
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
        assert_eq!(report.active, 0);
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
}
