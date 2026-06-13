//! Skill tools: skill_list and skill_view.
//!
//! Modeled after Hermes `tools/skills_tool.py`:
//! - `skill_list` — list all available skills with metadata (JSON response)
//! - `skill_view` — load a skill's full content by name (text response)

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use skill::SkillUsageStore;
use skill::utils::ReadinessStatus;

use skill::SkillRegistry;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};
use tool_core::Tool;

pub use loom_types::tools::tool_name::TOOL_SKILL_LIST;
pub use loom_types::tools::tool_name::TOOL_SKILL_VIEW;

const SKILLS_SUBDIR: &str = ".loom/skills";

pub(crate) struct SkillContext {
    registry: Option<Arc<SkillRegistry>>,
    working_folder: Option<Arc<std::path::PathBuf>>,
    usage_store: Option<SkillUsageStore>,
}

impl SkillContext {
    fn from_registry(registry: Arc<SkillRegistry>) -> Self {
        Self {
            registry: Some(registry),
            working_folder: None,
            usage_store: None,
        }
    }

    fn from_working_folder(working_folder: Arc<std::path::PathBuf>) -> Self {
        let store = {
            let skills_dir = working_folder.join(SKILLS_SUBDIR);
            SkillUsageStore::new(&skills_dir)
        };
        Self {
            registry: None,
            working_folder: Some(working_folder),
            usage_store: Some(store),
        }
    }

    fn with_store(mut self, store: SkillUsageStore) -> Self {
        self.usage_store = Some(store);
        self
    }

    fn skills_dir(&self) -> Option<std::path::PathBuf> {
        self.working_folder
            .as_ref()
            .map(|wf| wf.join(SKILLS_SUBDIR))
    }
}

// ---------------------------------------------------------------------------
// skill_list
// ---------------------------------------------------------------------------

pub struct SkillListTool {
    ctx: Arc<SkillContext>,
}

impl SkillListTool {
    pub(crate) fn new(ctx: Arc<SkillContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for SkillListTool {
    fn name(&self) -> &str {
        TOOL_SKILL_LIST
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_SKILL_LIST.to_string(),
            description: Some(
                "List all available skills with names, descriptions, and categories. \
                 Use this to discover which skills match your task before loading one with skill_view."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Optional category filter (e.g. \"coding\", \"debugging\")."
                    }
                }
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let category_filter = args
            .get("category")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if let Some(ref registry) = self.ctx.registry {
            let entries: Vec<&skill::SkillEntry> = registry.list().iter()
                .filter(|e| {
                    category_filter.as_ref().is_none_or(|cf| {
                        e.metadata.category.as_deref() == Some(cf.as_str())
                    })
                })
                .collect();

            let categories: Vec<String> = entries.iter()
                .filter_map(|e| e.metadata.category.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .into_iter()
                .collect();

            let mut categories_sorted = categories;
            categories_sorted.sort();

            let skills_json: Vec<serde_json::Value> = entries.iter()
                .map(|e| {
                    let mut obj = json!({
                        "name": e.metadata.name,
                        "description": e.metadata.description,
                    });
                    if let Some(ref cat) = e.metadata.category {
                        obj["category"] = json!(cat);
                    }
                    obj
                })
                .collect();

            let result = json!({
                "success": true,
                "skills": skills_json,
                "categories": categories_sorted,
                "count": entries.len(),
                "hint": "Use skill_view(name) to see full content"
            });

            return Ok(ToolCallContent::text(
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
            ));
        }

        let skills_dir = self.ctx.skills_dir().unwrap_or_else(|| {
            std::path::PathBuf::from(".loom/skills")
        });
        if !skills_dir.is_dir() {
            return Ok(ToolCallContent::text(
                json!({
                    "success": true,
                    "skills": [],
                    "categories": [],
                    "count": 0,
                    "hint": "No skills directory found"
                }).to_string()
            ));
        }

        let mut skills = Vec::new();
        let categories_set: std::collections::HashSet<String> = std::collections::HashSet::new();

        const EXTENSIONS: &[&str] = &["md", "txt", "markdown"];
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for e in entries.flatten() {
                let path = e.path();
                if path.is_file() {
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if !EXTENSIONS.contains(&ext) {
                        continue;
                    }
                    let name = path.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if name.eq_ignore_ascii_case("SKILL") || name.eq_ignore_ascii_case("DESCRIPTION") {
                        continue;
                    }
                    let content = match std::fs::read_to_string(&path) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let (meta_opt, _) = skill::utils::parse_skill_frontmatter(&content);
                    let description = meta_opt
                        .as_ref()
                        .map(|m| m.description.as_str())
                        .unwrap_or("")
                        .to_string();
                    skills.push(json!({
                        "name": name,
                        "description": description,
                    }));
                }
            }
        }

        let result = json!({
            "success": true,
            "skills": skills,
            "categories": categories_set.into_iter().collect::<Vec<_>>(),
            "count": skills.len(),
            "hint": "Use skill_view(name) to see full content"
        });

        Ok(ToolCallContent::text(
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
        ))
    }
}

// ---------------------------------------------------------------------------
// skill_view
// ---------------------------------------------------------------------------

pub struct SkillViewTool {
    ctx: Arc<SkillContext>,
}

impl SkillViewTool {
    pub(crate) fn new(ctx: Arc<SkillContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for SkillViewTool {
    fn name(&self) -> &str {
        TOOL_SKILL_VIEW
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_SKILL_VIEW.to_string(),
            description: Some(
                "Load a skill's full content by name. Use when a task matches one of the \
                 available skills. Optionally specify file_path to read a sub-file within \
                 the skill directory (e.g. \"references/api.md\", \"scripts/setup.sh\")."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Skill name to load (from skill_list or <available_skills>)."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Optional sub-file path within the skill directory \
                                        (e.g. \"references/api.md\", \"scripts/setup.sh\")."
                    }
                },
                "required": ["name"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing name".to_string()))?;

        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str());

        if let Some(ref registry) = self.ctx.registry {
            return self.view_from_registry(registry, name, file_path);
        }

        self.view_from_directory(name, file_path)
    }
}

impl SkillViewTool {
    fn resolve_name_with_ns<'a>(&self, name: &str, registry: &'a SkillRegistry) -> Result<Vec<&'a skill::SkillEntry>, ToolSourceError> {
        let matches: Vec<&'a skill::SkillEntry> = if let Some((ns, short_name)) = name.split_once(':') {
            registry.list().iter()
                .filter(|e| e.metadata.name == short_name && e.base_path.to_string_lossy().contains(ns))
                .collect()
        } else {
            registry.list().iter()
                .filter(|e| e.metadata.name == name)
                .collect()
        };
        Ok(matches)
    }

    fn view_from_registry(
        &self,
        registry: &SkillRegistry,
        name: &str,
        file_path: Option<&str>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let matches = self.resolve_name_with_ns(name, registry)?;

        if matches.is_empty() {
            let available: Vec<&str> = registry.list().iter()
                .map(|e| e.metadata.name.as_str())
                .collect();
            return Err(ToolSourceError::InvalidInput(format!(
                "skill '{}' not found. Available: {}",
                name,
                available.join(", ")
            )));
        }

        if matches.len() > 1 {
            let sources: Vec<String> = matches.iter()
                .map(|e| format!("{} ({:?})", e.base_path.display(), e.source))
                .collect();
            return Err(ToolSourceError::InvalidInput(format!(
                "Ambiguous skill '{}': found in {} locations. Use a more specific name or path.\n  {}",
                name,
                matches.len(),
                sources.join("\n  ")
            )));
        }

        let entry = matches[0];
        let base_path = &entry.base_path;

        if let Some(fp) = file_path {
            return self.view_sub_file(base_path, fp, name);
        }

        let content = std::fs::read_to_string(&entry.skill_file)
            .map_err(|source| ToolSourceError::Transport(
                format!("read skill {}: {}", name, source)
            ))?;
        let (_, body) = skill::utils::parse_frontmatter(&content);
        let mut out = body;

        if entry.skill_file.file_name().map(|f| f == "SKILL.md").unwrap_or(false) {
            if let Ok(rd) = std::fs::read_dir(base_path) {
                let subdirs: Vec<String> = rd
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                    .filter(|n| ["references", "templates", "scripts", "assets"].iter().any(|&d| n == d))
                    .collect();

                if !subdirs.is_empty() {
                    out.push_str("\n\n## Supporting directories\n");
                    for sd in &subdirs {
                        if let Ok(files) = std::fs::read_dir(base_path.join(sd)) {
                            let names: Vec<String> = files
                                .flatten()
                                .filter(|f| f.path().is_file())
                                .filter_map(|f| f.file_name().to_str().map(|s| s.to_string()))
                                .collect();
                            if !names.is_empty() {
                                out.push_str(&format!("- {}/: {}\n", sd, names.join(", ")));
                            }
                        }
                    }
                    out.push_str("Use file_path parameter to read individual files.\n");
                }
            }
        }

        let content = skill::substitute_template_vars(&out, base_path, None);

        let readiness_header = match entry.metadata.readiness_status() {
            ReadinessStatus::SetupNeeded(missing) => {
                format!(
                    "⚠️ readiness: setup_needed — missing env vars: {}\n\n",
                    missing.join(", ")
                )
            }
            ReadinessStatus::Unsupported(reason) => {
                format!("⚠️ readiness: unsupported — {}\n\n", reason)
            }
            ReadinessStatus::Available => String::new(),
        };

        if let Some(ref store) = self.ctx.usage_store {
            store.bump_view(name);
            store.bump_use(name);
        }

        Ok(ToolCallContent::text(format!(
            "<skill_content name=\"{}\">\n{}{}\n</skill_content>",
            name, readiness_header, content
        )))
    }

    fn view_sub_file(
        &self,
        skill_dir: &std::path::Path,
        file_path: &str,
        skill_name: &str,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let target = skill_dir.join(file_path);

        let canonical_skill = skill_dir.canonicalize().map_err(|e| {
            ToolSourceError::InvalidInput(format!("invalid skill dir: {}", e))
        })?;
        let canonical_target = target.canonicalize().map_err(|e| {
            ToolSourceError::InvalidInput(format!(
                "file '{}' not found in skill '{}': {}",
                file_path, skill_name, e
            ))
        })?;

        if !canonical_target.starts_with(&canonical_skill) {
            return Err(ToolSourceError::InvalidInput(format!(
                "path traversal: '{}' is outside skill directory",
                file_path
            )));
        }

        if canonical_target.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "'{}' is a directory, not a file",
                file_path
            )));
        }

        let content = std::fs::read_to_string(&canonical_target)
            .map_err(|e| ToolSourceError::Transport(
                format!("read {}: {}", file_path, e)
            ))?;

        if let Some(ref store) = self.ctx.usage_store {
            store.bump_view(skill_name);
            store.bump_use(skill_name);
        }

        Ok(ToolCallContent::text(format!(
            "<skill_content name=\"{}\" file=\"{}\">\n{}\n</skill_content>",
            skill_name, file_path, content
        )))
    }

    fn view_from_directory(
        &self,
        name: &str,
        file_path: Option<&str>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let skills_dir = self.ctx.skills_dir()
            .ok_or_else(|| ToolSourceError::InvalidInput("no working folder".to_string()))?;
        if !skills_dir.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "skills directory not found: {}",
                skills_dir.display()
            )));
        }

        const EXTENSIONS: &[&str] = &["md", "txt", "markdown"];
        for ext in EXTENSIONS {
            let p = skills_dir.join(format!("{}.{}", name, ext));
            if p.is_file() {
                let base_path = p.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();

                if let Some(fp) = file_path {
                    return self.view_sub_file(&base_path, fp, name);
                }

                let content = std::fs::read_to_string(&p)
                    .map_err(|e| ToolSourceError::Transport(format!("read skill: {}", e)))?;
                let content = skill::substitute_template_vars(&content, &base_path, None);
                if let Some(ref store) = self.ctx.usage_store {
                    store.bump_view(name);
                    store.bump_use(name);
                }
                return Ok(ToolCallContent::text(format!(
                    "<skill_content name=\"{}\">\n{}\n</skill_content>",
                    name, content
                )));
            }
        }

        let mut available = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for e in entries.flatten() {
                if let Some(stem) = e.path().file_stem() {
                    available.push(stem.to_string_lossy().to_string());
                }
            }
        }
        Err(ToolSourceError::InvalidInput(format!(
            "skill '{}' not found. Available: {}",
            name,
            available.join(", ")
        )))
    }
}

// ---------------------------------------------------------------------------
// Public constructors (used by register_file_tools)
// ---------------------------------------------------------------------------

pub fn make_skill_tools_with_registry(
    registry: Arc<SkillRegistry>,
    usage: Option<SkillUsageStore>,
) -> (SkillListTool, SkillViewTool) {
    let mut ctx = SkillContext::from_registry(registry);
    if let Some(store) = usage {
        ctx = ctx.with_store(store);
    }
    let ctx = Arc::new(ctx);
    (SkillListTool::new(ctx.clone()), SkillViewTool::new(ctx.clone()))
}

pub fn make_skill_tools_with_folder(
    working_folder: Arc<std::path::PathBuf>,
    usage: Option<SkillUsageStore>,
) -> (SkillListTool, SkillViewTool) {
    let mut ctx = SkillContext::from_working_folder(working_folder);
    if let Some(store) = usage {
        ctx = ctx.with_store(store);
    }
    let ctx = Arc::new(ctx);
    (SkillListTool::new(ctx.clone()), SkillViewTool::new(ctx.clone()))
}
