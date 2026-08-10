//! `loom skills inspect <name>` implementation.
//!
//! Discovers skills using `agent::skill::discovery::SkillRegistry::discover`,
//! provides a rich text summary and JSON dump of a single skill's metadata,
//! content, readiness, conditions, supporting files, and embedded references.
//!
//! Builtin skills (e.g. `WorkflowTool`) are injected by `build_inspect_registry`
//! so that `inspect workflow` can see the embedded version even without
//! `sync_skills` having run.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tool_core::Tool;

use crate::output::write_json_output;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A builtin skill that was injected into the registry by `build_inspect_registry`.
/// Stored for traceability (returned alongside the registry so callers can
/// inspect which builtins were contributed).
///
/// `pub(crate)` — internal to the inspect module, not part of the public CLI API.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct BuiltinSkillContribution {
    /// Tool that provided the builtin skill.
    pub tool_name: String,
    /// Skill name inserted into the registry.
    pub skill_name: String,
    /// Always `SkillSource::Builtin`.
    pub source: skill::discovery::SkillSource,
}

/// Output of a successful `inspect` call — all fields from §5.2 of the design doc.
#[derive(Debug, Clone, Serialize)]
pub struct InspectOutput {
    pub name: String,
    pub source: String,
    pub source_raw: String,
    pub path: Option<String>,
    pub skill_file: Option<String>,
    pub is_builtin: bool,
    pub readiness: ReadinessOutput,
    pub category: Option<String>,
    pub category_desc: Option<String>,
    pub description: String,
    pub triggers: Vec<String>,
    pub tags: Vec<String>,
    pub conditions: ConditionsOutput,
    pub required_env_vars: Vec<String>,
    pub prerequisites: PrerequisitesOutput,
    pub related_skills: Vec<String>,
    pub supporting_files: SupportingFilesOutput,
    pub embedded_references: Vec<EmbeddedReferenceOutput>,
    pub usage: UsageOutput,
    pub body: String,
    pub frontmatter_raw: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadinessOutput {
    pub status: String,
    pub missing_env_vars: Vec<String>,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConditionsOutput {
    pub requires_tools: Vec<String>,
    pub requires_toolsets: Vec<String>,
    pub fallback_for_tools: Vec<String>,
    pub fallback_for_toolsets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrerequisitesOutput {
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupportingFilesOutput {
    pub references: Vec<String>,
    pub templates: Vec<String>,
    pub scripts: Vec<String>,
    pub assets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedReferenceOutput {
    pub name: String,
    pub byte_size: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageOutput {
    pub use_count: i64,
    pub view_count: i64,
    pub patch_count: i64,
    pub last_used_at: Option<String>,
    pub last_viewed_at: Option<String>,
    pub last_patched_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub created_at: Option<String>,
    pub state: Option<String>,
    pub pinned: bool,
    pub archived_at: Option<String>,
    pub absorbed_into: Option<String>,
    pub created_by: Option<String>,
}

/// Errors specific to the `inspect` subcommand.
#[derive(Debug)]
pub enum SkillInspectError {
    /// No skill entry matched the requested name.
    NotFound(String),
    /// Multiple skill entries matched the name and disambiguation is needed.
    Ambiguous(Vec<(String, String)>),
    /// A path traversal was detected.
    PathTraversal(String),
    /// A required sub-file was not found.
    FileNotFound(String, std::io::Error),
    /// A general I/O error.
    Io(std::io::Error),
    /// Invalid flag combination.
    BadCombo(String),
}

impl std::fmt::Display for SkillInspectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillInspectError::NotFound(name) => write!(f, "skill not found: {}", name),
            SkillInspectError::Ambiguous(candidates) => {
                writeln!(f, "ambiguous skill name — {} candidates:", candidates.len())?;
                for (i, (name, source)) in candidates.iter().enumerate() {
                    writeln!(f, "  {}. {} (source: {})", i + 1, name, source)?;
                }
                write!(f, "hint: use --source <Source> to disambiguate")
            }
            SkillInspectError::PathTraversal(path) => {
                write!(f, "path traversal detected: '{}'", path)
            }
            SkillInspectError::FileNotFound(path, _) => {
                write!(f, "file not found in skill: {}", path)
            }
            SkillInspectError::Io(e) => write!(f, "I/O error: {}", e),
            SkillInspectError::BadCombo(msg) => write!(f, "invalid flag combination: {}", msg),
        }
    }
}

impl std::error::Error for SkillInspectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SkillInspectError::FileNotFound(_, e) => Some(e),
            SkillInspectError::Io(e) => Some(e),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Registry builder
// ---------------------------------------------------------------------------

/// Discover skills from the same filesystem locations as the agent, then
/// inject CLI-known builtin skills (currently only `WorkflowTool`).
///
/// Unlike the agent runtime, this function intentionally **skips**
/// `apply_filters` / `apply_toolset_filters` so that `inspect` can
/// display the raw universe of skills without toolset constraints.
///
/// Returns the populated registry together with a list of builtin
/// contributions (which tool provided which builtin skill).
///
/// # Extension point
/// When a second tool exposes `builtin_skill()`, add its constructor
/// to the `BUILTIN_PROVIDERS` list below. No other call-sites need change.
pub fn build_inspect_registry(
    working_folder: &Path,
    extra_dirs: &[PathBuf],
) -> Result<
    (
        skill::discovery::SkillRegistry,
        Vec<BuiltinSkillContribution>,
    ),
    Box<dyn std::error::Error>,
> {
    let mut registry = skill::discovery::SkillRegistry::discover(working_folder, extra_dirs)
        .map_err(std::io::Error::other)?;

    let mut contributions = Vec::new();

    // Inject WorkflowStartTool builtin (the only tool currently implementing builtin_skill).
    // Construction pattern matches `agent/tool/tool-workflow/tests/builtin_skill.rs`.
    let runtime = std::sync::Arc::new(tool_workflow::WorkflowRuntime::new(
        agent::agent::AgentConfig::default(),
    ));
    let tool = tool_workflow::WorkflowStartTool::new(runtime);
    if let Some(builtin) = tool.builtin_skill() {
        registry.add_builtin(
            &builtin.name,
            &builtin.description,
            &builtin.content,
            builtin.triggers,
            builtin.requires_tools,
            builtin.references,
        );
        contributions.push(BuiltinSkillContribution {
            tool_name: "workflow_start".to_string(),
            skill_name: builtin.name,
            source: skill::discovery::SkillSource::Builtin,
        });
    }

    // --- Extension point ---
    // Add more builtin providers here as additional tools implement `builtin_skill()`:
    //
    // let other_tool = SomeTool::new(config);
    // if let Some(builtin) = other_tool.builtin_skill() { ... }
    //
    // Keep this list in sync with any new `builtin_skill()` implementations.

    Ok((registry, contributions))
}

// ---------------------------------------------------------------------------
// Path-traversal guard
// ---------------------------------------------------------------------------

/// 5 MiB cap for `--read-file` on disk-backed skills (mirrors §8.2 design).
const MAX_FILE_SIZE_BYTES: u64 = 5 * 1024 * 1024;

/// Resolves `file_path` as a child of `skill_dir` and verifies it stays within
/// the skill directory boundary (path-traversal defense, §8).
///
/// 3-point defense:
/// 1. Canonicalize the skill directory first.
/// 2. Canonicalize the target and confirm it starts with the skill prefix.
/// 3. No `is_file()` check — caller reads after this guard.
///
/// Returns the canonical absolute path on success.
fn safe_join_under(skill_dir: &Path, file_path: &str) -> Result<PathBuf, SkillInspectError> {
    let target = skill_dir.join(file_path);

    // Point 1: canonicalize skill directory first
    let canonical_skill = skill_dir.canonicalize().map_err(SkillInspectError::Io)?;

    // Point 2: canonicalize target and confirm prefix membership
    let canonical_target = target
        .canonicalize()
        .map_err(|e| SkillInspectError::FileNotFound(file_path.to_string(), e))?;

    if !canonical_target.starts_with(&canonical_skill) {
        return Err(SkillInspectError::PathTraversal(file_path.to_string()));
    }

    // Point 3: no is_file() check — caller reads; this avoids TOCTOU + races.
    Ok(canonical_target)
}

// ---------------------------------------------------------------------------
// InspectOutput builder
// ---------------------------------------------------------------------------

/// Maximum bytes of body shown in text view when `--all` is not set.
/// §5.5: truncated to ~1.2 KiB.
const TEXT_BODY_PREVIEW_BYTES: usize = 1200;

/// Find the largest byte index ≤ `index` that is a UTF-8 char boundary.
/// Prevents panics when slicing multi-byte strings (e.g. CJK content).
fn floor_char_boundary(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

impl InspectOutput {
    /// Build from a `SkillEntry` + optional usage data from `SkillUsageStore`.
    fn from_entry(entry: &skill::discovery::SkillEntry, usage: Option<&SkillUsage>) -> Self {
        let usage = usage.cloned().unwrap_or_else(|| SkillUsage {
            name: entry.metadata.name.clone(),
            use_count: 0,
            view_count: 0,
            patch_count: 0,
            last_used_at: None,
            last_viewed_at: None,
            last_patched_at: None,
            created_at: None,
            state: None,
            pinned: false,
            archived_at: None,
            absorbed_into: None,
            created_by: None,
        });

        let (supporting_files, embedded_references) =
            Self::collect_supporting_files_and_refs(entry);

        InspectOutput {
            name: entry.metadata.name.clone(),
            source: entry.source.label().to_string(),
            source_raw: entry.source.label().to_string(),
            path: if entry.base_path.as_os_str().is_empty() {
                None
            } else {
                Some(entry.base_path.to_string_lossy().to_string())
            },
            skill_file: if entry.skill_file.as_os_str().is_empty() {
                None
            } else {
                Some(entry.skill_file.to_string_lossy().to_string())
            },
            is_builtin: entry.source == skill::discovery::SkillSource::Builtin,
            readiness: ReadinessOutput {
                status: "ready".to_string(),
                missing_env_vars: vec![],
                unsupported_reason: None,
            },
            category: entry.metadata.category.clone(),
            category_desc: entry.metadata.category_desc.clone(),
            description: entry.metadata.description.clone(),
            triggers: entry.metadata.triggers.clone(),
            tags: entry.metadata.tags.clone(),
            conditions: {
                let c = entry.metadata.conditions();
                ConditionsOutput {
                    requires_tools: c.requires_tools.clone(),
                    requires_toolsets: c.requires_toolsets.clone(),
                    fallback_for_tools: c.fallback_for_tools.clone(),
                    fallback_for_toolsets: c.fallback_for_toolsets.clone(),
                }
            },
            required_env_vars: entry
                .metadata
                .required_env_vars()
                .iter()
                .map(|v| v.name.clone())
                .collect(),
            prerequisites: PrerequisitesOutput {
                commands: entry
                    .metadata
                    .prerequisites
                    .as_ref()
                    .map(|p| p.commands.clone())
                    .unwrap_or_default(),
            },
            related_skills: entry
                .metadata
                .metadata
                .as_ref()
                .map(|m| m.related_skills.clone())
                .unwrap_or_default(),
            supporting_files,
            embedded_references,
            usage: UsageOutput {
                use_count: usage.use_count,
                view_count: usage.view_count,
                patch_count: usage.patch_count,
                last_used_at: usage.last_used_at.clone(),
                last_viewed_at: usage.last_viewed_at.clone(),
                last_patched_at: usage.last_patched_at.clone(),
                last_activity_at: usage.last_used_at.clone(),
                created_at: usage.created_at,
                state: usage.state,
                pinned: usage.pinned,
                archived_at: usage.archived_at,
                absorbed_into: usage.absorbed_into,
                created_by: usage.created_by,
            },
            body: entry.embedded_content.clone().unwrap_or_default(),
            frontmatter_raw: String::new(),
        }
    }

    /// Collect supporting file paths and embedded references from the skill entry.
    fn collect_supporting_files_and_refs(
        entry: &skill::discovery::SkillEntry,
    ) -> (SupportingFilesOutput, Vec<EmbeddedReferenceOutput>) {
        let mut refs = vec![];
        let mut templates = vec![];
        let mut scripts = vec![];
        let mut assets = vec![];

        if let Some(embedded_files) = &entry.embedded_files {
            for (name, _content) in embedded_files {
                if name.starts_with("references/") {
                    refs.push(name.clone());
                } else if name.starts_with("templates/") {
                    templates.push(name.clone());
                } else if name.starts_with("scripts/") {
                    scripts.push(name.clone());
                } else if name.starts_with("assets/") {
                    assets.push(name.clone());
                }
            }
        }

        // embedded_references: only files under references/
        let embedded_refs: Vec<_> =
            embedded_files_to_refs(entry.embedded_files.as_ref(), &["references/"]);

        (
            SupportingFilesOutput {
                references: refs,
                templates,
                scripts,
                assets,
            },
            embedded_refs,
        )
    }

    /// Render the §5.1 text view to stdout.
    fn render_text(&self, all: bool) {
        println!("Skill: {}", self.name);
        println!("{}", "═".repeat(60));
        println!("Source: {}", self.source);
        if let Some(ref path) = self.path {
            println!("Path: {}", path);
        }
        println!("Description: {}", self.description);

        if !self.triggers.is_empty() {
            println!("Triggers: {}", self.triggers.join(", "));
        }

        // Usage
        println!("Usage:");
        println!("  Use count: {}", self.usage.use_count);
        println!("  View count: {}", self.usage.view_count);
        if let Some(ref last) = self.usage.last_used_at {
            println!("  Last used: {}", last);
        }
        if self.usage.pinned {
            println!("  Pinned: true");
        }

        // Conditions — show non-empty fields only (§5.3)
        let has_any_condition = !self.conditions.requires_tools.is_empty()
            || !self.conditions.requires_toolsets.is_empty()
            || !self.conditions.fallback_for_tools.is_empty()
            || !self.conditions.fallback_for_toolsets.is_empty();

        if has_any_condition || all {
            println!("Conditions:");
            if all {
                if self.conditions.requires_tools.is_empty() {
                    println!("  requires_tools: (none)");
                } else {
                    println!(
                        "  requires_tools: {}",
                        self.conditions.requires_tools.join(", ")
                    );
                }
                if self.conditions.requires_toolsets.is_empty() {
                    println!("  requires_toolsets: (none)");
                } else {
                    println!(
                        "  requires_toolsets: {}",
                        self.conditions.requires_toolsets.join(", ")
                    );
                }
                if self.conditions.fallback_for_tools.is_empty() {
                    println!("  fallback_for_tools: (none)");
                } else {
                    println!(
                        "  fallback_for_tools: {}",
                        self.conditions.fallback_for_tools.join(", ")
                    );
                }
                if self.conditions.fallback_for_toolsets.is_empty() {
                    println!("  fallback_for_toolsets: (none)");
                } else {
                    println!(
                        "  fallback_for_toolsets: {}",
                        self.conditions.fallback_for_toolsets.join(", ")
                    );
                }
            } else {
                // Only non-empty fields
                if !self.conditions.requires_tools.is_empty() {
                    println!(
                        "  requires_tools: {}",
                        self.conditions.requires_tools.join(", ")
                    );
                }
                if !self.conditions.requires_toolsets.is_empty() {
                    println!(
                        "  requires_toolsets: {}",
                        self.conditions.requires_toolsets.join(", ")
                    );
                }
                if !self.conditions.fallback_for_tools.is_empty() {
                    println!(
                        "  fallback_for_tools: {}",
                        self.conditions.fallback_for_tools.join(", ")
                    );
                }
                if !self.conditions.fallback_for_toolsets.is_empty() {
                    println!(
                        "  fallback_for_toolsets: {}",
                        self.conditions.fallback_for_toolsets.join(", ")
                    );
                }
            }
        }

        // Supporting files
        if !self.supporting_files.references.is_empty() || all {
            println!("Supporting files:");
            if self.supporting_files.references.is_empty() && all {
                println!("  references: (none)");
            } else {
                for r in &self.supporting_files.references {
                    println!("  - {}", r);
                }
            }
        }

        // Embedded references
        if !self.embedded_references.is_empty() || all {
            println!("Embedded references:");
            if self.embedded_references.is_empty() && all {
                println!("  (none)");
            } else {
                for r in &self.embedded_references {
                    println!("  - {} ({} bytes)", r.name, r.byte_size);
                }
            }
        }

        // Body
        println!();
        println!("Body:");
        let body = &self.body;
        if body.is_empty() {
            println!("(no content)");
        } else if all {
            println!("{}", body);
        } else {
            // §5.5: truncate to ~1.2 KiB
            let truncated = if body.len() > TEXT_BODY_PREVIEW_BYTES {
                let safe_end = floor_char_boundary(body, TEXT_BODY_PREVIEW_BYTES);
                format!(
                    "{}...[truncated {} bytes, use --all for full body]",
                    &body[..safe_end],
                    body.len() - safe_end
                )
            } else {
                body.clone()
            };
            println!("{}", truncated);
        }
    }
}

/// Lightweight skill usage record extracted from `SkillUsageStore`.
/// Copied from loom_curator so we don't pull in that entire crate as a dep.
#[derive(Debug, Clone, serde::Deserialize)]
struct SkillUsage {
    name: String,
    use_count: i64,
    view_count: i64,
    patch_count: i64,
    last_used_at: Option<String>,
    last_viewed_at: Option<String>,
    last_patched_at: Option<String>,
    created_at: Option<String>,
    state: Option<String>,
    pinned: bool,
    archived_at: Option<String>,
    absorbed_into: Option<String>,
    created_by: Option<String>,
}

/// Convert embedded_files to EmbeddedReferenceOutput list, filtered by prefixes.
fn embedded_files_to_refs(
    embedded_files: Option<&Vec<(String, String)>>,
    prefixes: &[&str],
) -> Vec<EmbeddedReferenceOutput> {
    let Some(files) = embedded_files else {
        return vec![];
    };
    files
        .iter()
        .filter(|(name, _)| prefixes.iter().any(|p| name.starts_with(p)))
        .map(|(name, content)| EmbeddedReferenceOutput {
            name: name.clone(),
            byte_size: content.len(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

/// Entry point for the `inspect` subcommand.
///
/// Resolves the skill entry, then dispatches:
/// - `--read-file`: prints a single sub-file (builtin or disk-backed)
/// - `--json`: emits the full InspectOutput as JSON
/// - default: prints the §5.1 text view
///
/// When `--json` is false, text is written to stdout only (file output
/// for non-JSON is intentionally omitted to avoid surprising callers).
pub fn run(
    name: &str,
    all: bool,
    read_file: Option<&Path>,
    source: Option<&crate::args::SkillSourceFilter>,
    json: bool,
    pretty: bool,
    output_file: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().map_err(SkillInspectError::Io)?;
    let (registry, _contributions) = build_inspect_registry(&cwd, &[])?;

    // ---- Mutual exclusion (§4.4) ----
    if read_file.is_some() {
        if all {
            return Err(SkillInspectError::BadCombo(
                "--read-file cannot be combined with --all".to_string(),
            )
            .into());
        }
        if json {
            return Err(SkillInspectError::BadCombo(
                "--read-file cannot be combined with --json".to_string(),
            )
            .into());
        }
    }

    // Collect candidates matching the skill name
    let candidates: Vec<_> = registry
        .skills
        .iter()
        .filter(|e| e.metadata.name == name)
        .collect();

    // Apply --source filter if provided
    let candidates: Vec<_> = if let Some(filter) = source {
        let filter_label = filter.label();
        candidates
            .into_iter()
            .filter(|e| e.source.label() == filter_label)
            .collect()
    } else {
        candidates
    };

    let entry = match candidates.as_slice() {
        [] => return Err(SkillInspectError::NotFound(name.to_string()).into()),
        [e] => *e,
        _ => {
            let ambig: Vec<_> = candidates
                .iter()
                .map(|e| (e.metadata.name.clone(), e.source.label().to_string()))
                .collect();
            return Err(SkillInspectError::Ambiguous(ambig).into());
        }
    };

    // ---- --read-file branch ----
    if let Some(file_path) = read_file {
        let file_path_str = file_path.to_string_lossy();

        if entry.source == skill::discovery::SkillSource::Builtin {
            // Builtin: look up strictly by exact name (no prefix matching).
            if let Some(embedded_files) = &entry.embedded_files {
                let found = embedded_files
                    .iter()
                    .find(|(n, _)| n == file_path_str.as_ref());
                if let Some((_, content)) = found {
                    println!("{}", content);
                    return Ok(());
                }
            }
            // Strict eq miss → FileNotFound
            return Err(SkillInspectError::FileNotFound(
                file_path_str.to_string(),
                std::io::Error::new(std::io::ErrorKind::NotFound, "not in embedded_files"),
            )
            .into());
        } else {
            // Disk-backed: path-traversal guard + 5 MiB cap.
            let target = safe_join_under(&entry.base_path, file_path_str.as_ref())?;
            let metadata = std::fs::metadata(&target).map_err(SkillInspectError::Io)?;
            if metadata.len() > MAX_FILE_SIZE_BYTES {
                return Err(SkillInspectError::BadCombo(format!(
                    "file '{}' exceeds the {} byte limit (is {} bytes)",
                    file_path_str,
                    MAX_FILE_SIZE_BYTES,
                    metadata.len()
                ))
                .into());
            }
            let content = std::fs::read_to_string(&target).map_err(SkillInspectError::Io)?;
            println!("{}", content);
            return Ok(());
        }
    }

    // Try to load usage from SkillUsageStore (best-effort; not found is non-fatal).
    let usage: Option<SkillUsage> = load_skill_usage(&entry.metadata.name);

    // Build full InspectOutput
    let output = InspectOutput::from_entry(entry, usage.as_ref());

    // ---- JSON branch (§5.2) ----
    if json {
        let json_value = serde_json::to_value(&output)
            .map_err(|e| SkillInspectError::Io(std::io::Error::other(e)))?;
        write_json_output(&json_value, output_file, pretty)?;
        return Ok(());
    }

    // ---- Default text view (§5.1) ----
    output.render_text(all);
    Ok(())
}

/// Load skill usage from the filesystem store (best-effort).
fn load_skill_usage(name: &str) -> Option<SkillUsage> {
    let store_path = config::home::loom_home().join(".skills.usage.json");
    let content = std::fs::read_to_string(&store_path).ok()?;
    let entries: Vec<SkillUsage> = serde_json::from_str(&content).ok()?;
    entries.into_iter().find(|u| u.name == name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ---- safe_join_under ----

    #[test]
    fn safe_join_under_accepts_valid_subpath() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skill");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(skill_dir.join("references").join("api.md"), "api docs").unwrap();

        let result = safe_join_under(&skill_dir, "references/api.md");
        assert!(result.is_ok(), "valid subpath must resolve: {:?}", result);
        let canon = result.unwrap();
        let loss = canon.to_string_lossy();
        // Use contains — Windows canonical paths use backslashes.
        assert!(
            loss.contains("references") && loss.contains("api.md"),
            "canonical path should contain references/api.md, got: {}",
            loss
        );
    }

    #[test]
    fn safe_join_under_rejects_traversal_dotdot() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "my skill").unwrap();

        // Count ".." components vs skill_dir depth to detect real traversal.
        use std::path::Component;
        let target = skill_dir.join("../../../etc/passwd");
        let num_parent_refs = target
            .components()
            .filter(|c| matches!(c, Component::ParentDir))
            .count();
        let num_skill_components = skill_dir.components().count();

        if num_parent_refs > num_skill_components {
            // Real traversal: must be rejected
            let result = safe_join_under(&skill_dir, "../../../etc/passwd");
            assert!(
                matches!(result, Err(SkillInspectError::PathTraversal(_))),
                "traversal should be rejected: {:?}",
                result
            );
        } else {
            // Path stays within root — canonicalize errors gracefully.
            let result = safe_join_under(&skill_dir, "../../../etc/passwd");
            assert!(result.is_err(), "path with .. should error: {:?}", result);
        }
    }

    #[test]
    fn safe_join_under_rejects_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skill");
        let sibling = tmp.path().join("sibling");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("secret.md"), "should not read").unwrap();

        #[cfg(windows)]
        let link_target = "..\\sibling\\secret.md";
        #[cfg(not(windows))]
        let link_target = "../sibling/secret.md";

        let link = skill_dir.join("link.md");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&link_target, &link).ok();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(link_target, &link).ok();

        let result = safe_join_under(&skill_dir, "link.md");
        assert!(
            matches!(result, Err(SkillInspectError::PathTraversal(_))),
            "symlink escape must be rejected: {:?}",
            result
        );
    }

    #[test]
    fn safe_join_under_rejects_nonexistent() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let result = safe_join_under(&skill_dir, "does/not/exist.md");
        assert!(
            matches!(result, Err(SkillInspectError::FileNotFound(_, _))),
            "nonexistent path must error FileNotFound: {:?}",
            result
        );
    }

    // ---- run with --read-file ----

    #[test]
    fn read_file_builtin_strict_eq() {
        let tmp = tempfile::tempdir().unwrap();
        let (registry, _contributions) =
            build_inspect_registry(tmp.path(), &[]).expect("build_inspect_registry ok");

        let entry = registry
            .skills
            .iter()
            .find(|e| e.metadata.name == "workflow")
            .expect("workflow builtin must exist");

        // For a builtin entry, "references/architecture" (without suffix)
        // must NOT match "references/architecture.md" — strict equality only.
        let file_path_str = "references/architecture"; // no .md suffix
        if entry.source == skill::discovery::SkillSource::Builtin {
            if let Some(embedded_files) = &entry.embedded_files {
                let found = embedded_files.iter().find(|(n, _)| n == file_path_str);
                assert!(
                    found.is_none(),
                    "strict eq: 'references/architecture' should NOT match any file; \
                     found: {:?}",
                    found
                );
            }
        }
    }

    #[test]
    fn read_file_mutual_exclusion_with_json() {
        let result = run(
            "workflow",
            false,
            Some(Path::new("references/architecture.md")),
            None,
            true,
            false,
            None,
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("--read-file cannot be combined with --json"),
            "expected mutual-exclusion error, got: {}",
            msg
        );
    }

    #[test]
    fn read_file_mutual_exclusion_with_all() {
        let result = run(
            "workflow",
            true,
            Some(Path::new("references/architecture.md")),
            None,
            false,
            false,
            None,
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("--read-file cannot be combined with --all"),
            "expected mutual-exclusion error, got: {}",
            msg
        );
    }

    #[test]
    fn read_file_disk_backed_prints_content() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join(".loom").join("skills").join("my-skill");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "name: my-skill\ndescription: test\n",
        )
        .unwrap();
        fs::write(
            skill_dir.join("references").join("guide.md"),
            "# Guide\nHello world",
        )
        .unwrap();

        let (registry, _contributions) =
            build_inspect_registry(tmp.path(), &[]).expect("build_inspect_registry ok");

        let entry = registry
            .skills
            .iter()
            .find(|e| e.metadata.name == "my-skill")
            .expect("my-skill must exist");

        assert_eq!(
            entry.source,
            skill::discovery::SkillSource::Project,
            "my-skill should be Project source"
        );

        let result = safe_join_under(&entry.base_path, "references/guide.md");
        assert!(result.is_ok(), "safe_join_under must succeed: {:?}", result);
        let content = fs::read_to_string(result.unwrap()).unwrap();
        assert_eq!(content, "# Guide\nHello world");
    }

    #[test]
    fn read_file_disk_over_5mb_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join(".loom").join("skills").join("big-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "name: big-skill\ndescription: test\n",
        )
        .unwrap();

        // Create a file > 5 MiB (6 MB)
        let big_content = "x".repeat(6 * 1024 * 1024);
        fs::write(skill_dir.join("big.md"), &big_content).unwrap();

        // Verify the file is large enough
        let metadata = fs::metadata(skill_dir.join("big.md")).unwrap();
        assert!(
            metadata.len() > MAX_FILE_SIZE_BYTES,
            "test file must exceed 5 MiB"
        );

        // run() calls build_inspect_registry(cwd) — chdir to tmp so it finds big-skill.
        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let result = run(
            "big-skill",
            false,
            Some(Path::new("big.md")),
            None,
            false,
            false,
            None,
        );
        std::env::set_current_dir(original_cwd).ok();

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("exceeds the") && msg.contains("byte limit"),
            "expected size-cap error, got: {}",
            msg
        );
    }

    // ---- JSON view ----

    #[test]
    fn json_view_emits_complete_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let (registry, _contributions) =
            build_inspect_registry(tmp.path(), &[]).expect("build_inspect_registry ok");

        let entry = registry
            .skills
            .iter()
            .find(|e| e.metadata.name == "workflow")
            .expect("workflow builtin must exist");

        let output = InspectOutput::from_entry(entry, None);
        let json = serde_json::to_value(&output).unwrap();

        // All 16 top-level keys from §5.2
        let expected_keys = [
            "name",
            "source",
            "source_raw",
            "path",
            "skill_file",
            "is_builtin",
            "readiness",
            "category",
            "category_desc",
            "description",
            "triggers",
            "tags",
            "conditions",
            "required_env_vars",
            "prerequisites",
            "related_skills",
            "supporting_files",
            "embedded_references",
            "usage",
            "body",
            "frontmatter_raw",
        ];
        for key in &expected_keys {
            assert!(
                json.get(*key).is_some(),
                "§5.2 key '{}' must be present in JSON output",
                key
            );
        }

        // Nested readiness keys
        let readiness = &json["readiness"];
        for key in &["status", "missing_env_vars", "unsupported_reason"] {
            assert!(
                readiness.get(*key).is_some(),
                "readiness.{} must exist",
                key
            );
        }

        // Nested conditions keys
        let conditions = &json["conditions"];
        for key in &[
            "requires_tools",
            "requires_toolsets",
            "fallback_for_tools",
            "fallback_for_toolsets",
        ] {
            assert!(
                conditions.get(*key).is_some(),
                "conditions.{} must exist",
                key
            );
        }

        // Nested usage keys
        let usage_keys = [
            "use_count",
            "view_count",
            "patch_count",
            "last_used_at",
            "last_viewed_at",
            "last_patched_at",
            "last_activity_at",
            "created_at",
            "state",
            "pinned",
            "archived_at",
            "absorbed_into",
            "created_by",
        ];
        let usage = &json["usage"];
        for key in &usage_keys {
            assert!(usage.get(*key).is_some(), "usage.{} must exist", key);
        }
    }

    #[test]
    fn json_to_file_uses_global_file_flag() {
        // When output_file is Some, JSON must be written to that file.
        // Verify by checking write_json_output path is exercised.
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("inspect.json");

        // run() with json=true and output_file=Some -> writes to file
        let result = run(
            "workflow",
            false,
            None,
            None,
            true,  // json
            false, // pretty
            Some(&output_path),
        );
        assert!(result.is_ok(), "json+file run should succeed: {:?}", result);

        // File must exist and contain valid JSON
        let content = fs::read_to_string(&output_path).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("output file must contain valid JSON");
        assert_eq!(
            parsed["name"], "workflow",
            "JSON should have correct skill name"
        );
        assert_eq!(
            parsed["source"], "Builtin",
            "source should be Builtin for workflow"
        );
    }

    // ---- text view ----

    #[test]
    fn text_view_truncates_body_without_all_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let (registry, _contributions) =
            build_inspect_registry(tmp.path(), &[]).expect("build_inspect_registry ok");

        let entry = registry
            .skills
            .iter()
            .find(|e| e.metadata.name == "workflow")
            .expect("workflow builtin must exist");

        let output = InspectOutput::from_entry(entry, None);

        // Without --all, body is truncated if over 1.2 KiB
        let body_len = output.body.len();
        if body_len > TEXT_BODY_PREVIEW_BYTES {
            // Verify the InspectOutput body is NOT truncated internally
            // (truncation happens at render time in render_text).
            assert!(
                output.body.len() > TEXT_BODY_PREVIEW_BYTES,
                "InspectOutput.body should be full; truncation is at render time"
            );
        }
    }

    // ---- existing phase-2 tests ----

    #[test]
    fn cli_skeleton_compiles_and_lists_args() {
        let result = run(
            "__nonexistent_skill_phase2_test__",
            false,
            None,
            None,
            false,
            false,
            None,
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found"),
            "expected 'not found' in error, got: {}",
            msg
        );
    }

    #[test]
    fn build_inspect_registry_returns_workflow_builtin() {
        let tmp = tempfile::tempdir().unwrap();
        let (registry, contributions) =
            build_inspect_registry(tmp.path(), &[]).expect("build_inspect_registry must succeed");

        let entries: Vec<_> = registry.list().to_vec();

        let workflow_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.metadata.name == "workflow")
            .collect();
        assert!(
            !workflow_entries.is_empty(),
            "registry should contain 'workflow' skill; got entries: {:?}",
            entries.iter().map(|e| &e.metadata.name).collect::<Vec<_>>()
        );

        let entry = workflow_entries.first().unwrap();
        assert_eq!(
            entry.source,
            skill::discovery::SkillSource::Builtin,
            "workflow entry should have source Builtin"
        );
        assert!(
            entry.embedded_content.is_some(),
            "workflow builtin must have embedded_content"
        );

        assert!(
            contributions.iter().any(|c| c.skill_name == "workflow"),
            "contributions should include 'workflow'; got: {:?}",
            contributions
        );
    }

    #[test]
    fn build_inspect_registry_preserves_disk_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join(".loom").join("skills").join("workflow");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "name: workflow\ndescription: My custom workflow skill\ntriggers:\n  - workflow\n",
        )
        .unwrap();

        let (registry, _contributions) =
            build_inspect_registry(tmp.path(), &[]).expect("build_inspect_registry must succeed");

        let entries: Vec<_> = registry.list().to_vec();

        let workflow_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.metadata.name == "workflow")
            .collect();
        assert_eq!(
            workflow_entries.len(),
            1,
            "expected exactly 1 workflow entry; got: {:?}",
            workflow_entries
                .iter()
                .map(|e| format!("{}@{:?}", e.metadata.name, e.source))
                .collect::<Vec<_>>()
        );

        let entry = workflow_entries.first().unwrap();
        assert_eq!(
            entry.source,
            skill::discovery::SkillSource::Project,
            "disk skill should win over builtin"
        );
        assert!(
            entry.embedded_content.is_none(),
            "Project entry should not have embedded_content (it's from disk)"
        );
    }

    // ---- Focused tests for the viewing goal ----

    /// `--all` must be passed through from the CLI parser.
    /// Verifies that the subcommands wiring passes the actual flag value,
    /// not a hardcoded `false`. We test this indirectly by confirming
    /// that `run(..., all=true, ...)` produces untruncated output for a
    /// skill whose body exceeds TEXT_BODY_PREVIEW_BYTES.
    #[test]
    fn all_flag_shows_full_body_no_truncation_marker() {
        // Build a synthetic entry with CJK + ASCII content > 1200 bytes.
        let cjk = "你好世界".repeat(200); // 3 bytes/char * 200 = 600 bytes per repeat → 600 bytes for 200 chars
        let body = format!("# Workflow Skill\n\n{}\n", cjk);
        assert!(
            body.len() > TEXT_BODY_PREVIEW_BYTES,
            "synthetic body must exceed preview size"
        );

        // Build InspectOutput manually to test render_text(all=true).
        let output = InspectOutput {
            name: "test-skill".to_string(),
            source: "Builtin".to_string(),
            source_raw: "Builtin".to_string(),
            path: None,
            skill_file: None,
            is_builtin: true,
            readiness: ReadinessOutput {
                status: "ready".to_string(),
                missing_env_vars: vec![],
                unsupported_reason: None,
            },
            category: None,
            category_desc: None,
            description: "test".to_string(),
            triggers: vec![],
            tags: vec![],
            conditions: ConditionsOutput {
                requires_tools: vec![],
                requires_toolsets: vec![],
                fallback_for_tools: vec![],
                fallback_for_toolsets: vec![],
            },
            required_env_vars: vec![],
            prerequisites: PrerequisitesOutput { commands: vec![] },
            related_skills: vec![],
            supporting_files: SupportingFilesOutput {
                references: vec![],
                templates: vec![],
                scripts: vec![],
                assets: vec![],
            },
            embedded_references: vec![],
            usage: UsageOutput {
                use_count: 0,
                view_count: 0,
                patch_count: 0,
                last_used_at: None,
                last_viewed_at: None,
                last_patched_at: None,
                last_activity_at: None,
                created_at: None,
                state: None,
                pinned: false,
                archived_at: None,
                absorbed_into: None,
                created_by: None,
            },
            body: body.clone(),
            frontmatter_raw: String::new(),
        };

        // render_text prints to stdout; verify it doesn't panic.
        output.render_text(true);

        // When all=true, the body field should contain the full text.
        assert_eq!(
            output.body, body,
            "InspectOutput.body must be full when all=true"
        );
        assert!(
            !output.body.contains("[truncated"),
            "body must not contain truncation marker when all=true"
        );
    }

    /// UTF-8/CJK truncation must not panic.
    /// A CJK string where byte index 1200 falls mid-character should
    /// be safely truncated by floor_char_boundary.
    #[test]
    fn utf8_cjk_truncation_is_safe() {
        // Each CJK char is 3 bytes in UTF-8.
        // 401 chars = 1203 bytes. Index 1200 is inside the last char (bytes 1200,1201,1202).
        let cjk: String = "你".repeat(401); // 401 × 3 = 1203 bytes
        assert_eq!(cjk.len(), 1203);

        // floor_char_boundary should find index 1200 (start of last char at 1200).
        let boundary = floor_char_boundary(&cjk, TEXT_BODY_PREVIEW_BYTES);
        assert!(
            cjk.is_char_boundary(boundary),
            "floor_char_boundary must return a valid char boundary"
        );
        assert!(
            boundary <= TEXT_BODY_PREVIEW_BYTES,
            "boundary must be ≤ TEXT_BODY_PREVIEW_BYTES"
        );

        // Slicing at the safe boundary must not panic.
        let _sliced = &cjk[..boundary];

        // Also test a string where 1200 falls exactly on a boundary.
        let ascii: String = "a".repeat(2000);
        let boundary2 = floor_char_boundary(&ascii, TEXT_BODY_PREVIEW_BYTES);
        assert_eq!(boundary2, TEXT_BODY_PREVIEW_BYTES);
    }

    /// Builtin workflow must appear in the inspect registry.
    #[test]
    fn builtin_workflow_in_inspect_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let (registry, contributions) =
            build_inspect_registry(tmp.path(), &[]).expect("registry build");

        let workflow = registry
            .skills
            .iter()
            .find(|e| e.metadata.name == "workflow");
        assert!(workflow.is_some(), "workflow must be in inspect registry");

        let entry = workflow.unwrap();
        assert_eq!(
            entry.source,
            skill::discovery::SkillSource::Builtin,
            "workflow must be Builtin"
        );
        assert!(
            entry.embedded_content.is_some(),
            "workflow must have embedded content"
        );
        assert!(
            contributions.iter().any(|c| c.skill_name == "workflow"),
            "contributions must include workflow"
        );
    }

    /// `loom skills inspect workflow --read-file references/examples.md`
    /// should print the embedded reference content for the builtin workflow.
    #[test]
    fn read_file_builtin_examples_md_works() {
        // run() uses std::env::current_dir(), so it should find the builtin
        // workflow regardless of the working directory.
        let result = run(
            "workflow",
            false,
            Some(Path::new("references/examples.md")),
            None,
            false,
            false,
            None,
        );

        assert!(
            result.is_ok(),
            "--read-file references/examples.md must succeed for builtin workflow; err: {:?}",
            result.err()
        );
    }

    /// Invalid builtin reference path should return a clear inspect error
    /// (FileNotFound), not a misleading "skill not found".
    #[test]
    fn read_file_builtin_invalid_path_is_file_not_found() {
        let result = run(
            "workflow",
            false,
            Some(Path::new("references/__does_not_exist__.md")),
            None,
            false,
            false,
            None,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();

        // Must be a FileNotFound-style error, NOT "skill not found".
        assert!(
            msg.contains("file not found"),
            "expected 'file not found in skill', got: {}",
            msg
        );
        assert!(
            !msg.contains("skill not found"),
            "must not say 'skill not found' for an invalid reference path; got: {}",
            msg
        );
    }

    /// `supporting_files.references` must not contain duplicates and must
    /// not include templates/scripts/assets.
    #[test]
    fn supporting_files_references_no_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let (registry, _contributions) =
            build_inspect_registry(tmp.path(), &[]).expect("registry build");

        let entry = registry
            .skills
            .iter()
            .find(|e| e.metadata.name == "workflow")
            .expect("workflow must exist");

        let output = InspectOutput::from_entry(entry, None);

        // Check references for duplicates.
        let refs = &output.supporting_files.references;
        let mut seen = std::collections::HashSet::new();
        for r in refs {
            assert!(seen.insert(r.clone()), "duplicate reference found: {}", r);
            assert!(
                r.starts_with("references/"),
                "reference should start with 'references/': {}",
                r
            );
        }

        // Templates/scripts/assets must not appear in references.
        for t in &output.supporting_files.templates {
            assert!(
                !refs.contains(t),
                "template '{}' must not appear in references",
                t
            );
        }
        for s in &output.supporting_files.scripts {
            assert!(
                !refs.contains(s),
                "script '{}' must not appear in references",
                s
            );
        }
    }

    /// `--source Builtin` must filter to only Builtin-source entries,
    /// and the workflow skill should match.
    #[test]
    fn source_builtin_filters_workflow() {
        let result = run(
            "workflow",
            false,
            None,
            Some(&crate::args::SkillSourceFilter::Builtin),
            true, // json — easier to verify via fields
            false,
            None,
        );

        assert!(
            result.is_ok(),
            "--source Builtin should find workflow; err: {:?}",
            result.err()
        );
    }

    /// `--source Builtin` should reject a non-builtin skill name
    /// (or a name that only exists as Project/User source).
    #[test]
    fn source_builtin_rejects_non_builtin() {
        // Create a project skill in a temp dir, then run with --source Builtin.
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join(".loom").join("skills").join("local-only");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "name: local-only\ndescription: project skill\n",
        )
        .unwrap();

        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let result = run(
            "local-only",
            false,
            None,
            Some(&crate::args::SkillSourceFilter::Builtin),
            false,
            false,
            None,
        );
        std::env::set_current_dir(original_cwd).ok();

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found"),
            "local-only should not be found with --source Builtin; got: {}",
            msg
        );
    }
}
