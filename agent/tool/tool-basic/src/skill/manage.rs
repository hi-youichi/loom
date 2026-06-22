//! `SkillManagerTool` — combined 6-action skill write tool, byte-for-byte
//! identical schema to the reference implementation's `skill_manage`.
//!
//! See `thirdparty/reference-agent/tools/skill_manager_tool.py::SKILL_MANAGE_SCHEMA`.
//!
//! # Schema parity
//!
//! - **Tool name**: `skill_manage`
//! - **Actions enum**: `["create", "patch", "edit", "delete", "write_file", "remove_file"]`
//! - **Required**: `["action", "name"]`
//! - **Properties**: `action`, `name`, `content`, `old_string`, `new_string`,
//!   `replace_all`, `category`, `file_path`, `file_content`, `absorbed_into`
//! - **Per-property descriptions**: copied verbatim from the reference schema.
//!
//! Any field not in the reference schema (`description`, `triggers`) is **not
//! exposed** in the tool schema and **not consumed** by the handler. Skills
//! always carry their metadata inside the SKILL.md frontmatter.
//!
//! # Action semantics
//!
//! - **`create`**: Take a `content` argument containing the full SKILL.md
//!   (YAML frontmatter + markdown body). Parse the frontmatter to extract
//!   `name` and `description`. Write to `Source::Auto` (background review
//!   context also calls `SkillUsageStore::mark_agent_created`).
//! - **`edit`**: Replace the SKILL.md with a new full document passed via
//!   `content`. Same frontmatter validation as `create`.
//! - **`patch`**: `old_string` / `new_string` find-and-replace. Validates
//!   the result and reverts to a snapshot on failure. `replace_all=true`
//!   replaces every occurrence; default is unique match.
//! - **`delete`**: Remove a skill. Requires `is_agent_created == true` if a
//!   `SkillUsageStore` is configured. `absorbed_into` is a three-state
//!   field: `None` = plain delete, `""` = archive, `"<name>"` = merged into
//!   target.
//! - **`write_file`**: Add or overwrite a support file. `file_content` is
//!   the file body. Path validated against traversal / absolute / backslash.
//! - **`remove_file`**: Delete a support file. Path validated.
//!
//! # WriteOrigin
//!
//! The tool is bound at construction time to a specific `WriteOrigin`
//! (either `Foreground` or `BackgroundReview`) via the `for_foreground` or
//! `for_background_review` factory methods. The `WriteOriginGuard` is set
//! on every `call()` so that any code reading `WriteOrigin::current()`
//! during the call sees the bound origin.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tool_core::{Tool, ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

use skill::provenance::{WriteOrigin, WriteOriginGuard};
use skill::storage::{Lifecycle, SkillContent, SkillError, SkillStorageRegistry, Source};
use skill::usage::SkillUsageStore;
use skill::security;
use skill::validation::{validate_skill_create, validate_skill_name, validate_skill_path, validate_frontmatter, validate_name_match, Severity, ValidationWarning};

pub const TOOL_SKILL_MANAGE: &str = "skill_manage";

/// Maximum size for a single support file (1 MiB).
const MAX_SKILL_FILE_BYTES: usize = 1_048_576;

/// Validate a category name.
///
/// Returns `None` if valid, or `Some(error_message)` if invalid.
fn validate_category(category: Option<&str>) -> Option<String> {
    if let Some(cat) = category {
        if cat.is_empty() {
            return Some("category must not be empty.".to_string());
        }
        if cat.chars().count() > 64 {
            return Some(format!("category must be at most 64 characters (got {}).", cat.chars().count()));
        }
        if !cat.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/') {
            return Some("category may only contain alphanumeric, '-', '_', '/' characters.".to_string());
        }
    }
    None
}

/// Tool description — copied verbatim from the reference schema.
///
/// Kept as a multi-line `const` using `\` line-continuation so reviewers can
/// see the full prompt text in the source. The `\` at end-of-line joins
/// the next line without inserting a newline; literal `\n` sequences are
/// preserved. The string is passed to the LLM verbatim via
/// `ToolSpec::description`.
const DESCRIPTION: &str = "Manage skills (create, update, delete). \
Skills are your procedural memory — reusable approaches for recurring task \
types. New skills go to <LOOM_HOME>/skills/; existing skills can be \
modified wherever they live.\n\
\n\
Actions: create (full SKILL.md + optional category), \
patch (old_string/new_string — preferred for fixes), \
edit (full SKILL.md rewrite — major overhauls only), \
delete, write_file, remove_file.\n\
\n\
On delete, pass `absorbed_into=<umbrella>` when you're merging this \
skill's content into another one, or `absorbed_into=\"\"` when you're pruning \
it with no forwarding target. This lets the curator tell consolidation from \
pruning without guessing, so downstream consumers (cron jobs that reference \
the old skill name, etc.) get updated correctly. The target you name in \
`absorbed_into` must already exist — create/patch the umbrella first, then \
delete.\n\
\n\
Create when: complex task succeeded (5+ calls), errors overcome, \
user-corrected approach worked, non-trivial workflow discovered, or user \
asks you to remember a procedure. Update when: instructions stale/wrong, \
OS-specific failures, missing steps or pitfalls found during use. If you \
used a skill and hit issues not covered by it, patch it immediately.\n\
\n\
After difficult/iterative tasks, offer to save as a skill. Skip for \
simple one-offs. Confirm with user before creating/deleting.\n\
\n\
Good skills: trigger conditions, numbered steps with exact commands, \
pitfalls section, verification steps. Use skill_view() to see format examples.\n\
\n\
Pinned skills are protected from deletion only — \
skill_manage(action='delete') will refuse with a message pointing the user \
to `loom curator unpin <name>`. Patches and edits go through on pinned \
skills so you can still improve them as pitfalls come up; pin only guards \
against irrecoverable loss.";

pub struct SkillManagerTool {
    storage: Arc<SkillStorageRegistry>,
    usage: Option<Arc<SkillUsageStore>>,
    default_origin: WriteOrigin,
}

impl SkillManagerTool {
    pub fn for_background_review(
        storage: Arc<SkillStorageRegistry>,
        usage: Option<Arc<SkillUsageStore>>,
    ) -> Self {
        Self {
            storage,
            usage,
            default_origin: WriteOrigin::BackgroundReview,
        }
    }

    pub fn for_foreground(
        storage: Arc<SkillStorageRegistry>,
        usage: Option<Arc<SkillUsageStore>>,
    ) -> Self {
        Self {
            storage,
            usage,
            default_origin: WriteOrigin::Foreground,
        }
    }

    async fn handle_create(&self, args: &Value) -> Result<Value, ToolSourceError> {
        let name = require_str(args, "name")?;
        let raw_content = require_str(args, "content")?;
        let category = args.get("category").and_then(|v| v.as_str());

        if let Some(err) = validate_category(category) {
            return Ok(error_response(&err));
        }

        let name_validation = validate_skill_name(name);
        if !name_validation.valid {
            return Ok(error_response(&format_critical_warnings(
                &name_validation.warnings,
            )));
        }

        let (parsed_name, description, triggers, body) = match validate_frontmatter(raw_content) {
            Ok(t) => t,
            Err(v) => return Ok(error_response(&format_critical_warnings(&v.warnings))),
        };

        let name_match = validate_name_match(&parsed_name, name);
        if !name_match.valid {
            return Ok(error_response(&format_critical_warnings(&name_match.warnings)));
        }

        let content = SkillContent {
            name: name.to_string(),
            description: description.clone(),
            triggers,
            lifecycle: Lifecycle::Active,
            source: Source::Auto,
            created_by: Some("agent".to_string()),
            body,
            raw: String::new(),
        };

        let validation = validate_skill_create(&content);
        if !validation.valid {
            return Ok(error_response(&format!(
                "Validation failed: {}",
                format_critical_warnings(&validation.warnings)
            )));
        }

        if self.storage.load(name).is_ok() {
            let existing_dir = self.storage.skill_dir(Source::Auto, name);
            return Ok(error_response(&format!(
                "A skill named '{}' already exists at {}. Use action='edit' to update it, \
                 or action='patch' for in-place changes.",
                name, existing_dir.display()
            )));
        }

        self.storage.save(name, &content).map_err(map_skill_err)?;

        let base_dir = self.storage.base_dir();
        let skill_dir = self.storage.skill_dir(Source::Auto, name);
        let skill_md_path = skill_dir.join("SKILL.md");

        if let Err(report) = security::security_scan_skill(&skill_dir) {
            let _ = self.storage.delete(name);
            return Ok(error_response(&report));
        }

        // Mark as agent-created only after security scan passes, so we don't
        // leave orphan agent_created state on a rolled-back skill.
        if matches!(self.default_origin, WriteOrigin::BackgroundReview) {
            if let Some(ref usage) = self.usage {
                usage.mark_agent_created(name);
            }
        }

        let rel_path = skill_dir.strip_prefix(base_dir).unwrap_or(&skill_dir);
        let path_str = rel_path.to_string_lossy().to_string();
        let skill_md_str = skill_md_path.to_string_lossy().to_string();

        let mut response = json!({
            "success": true,
            "message": format!("Skill '{}' created.", name),
            "path": path_str,
            "skill_md": skill_md_str,
            "hint": format!(
                "To add reference files, templates, or scripts, use \
                 skill_manage(action='write_file', name='{}', \
                 file_path='references/example.md', file_content='...')",
                name
            ),
        });
        if let Some(cat) = category {
            response["category"] = json!(cat);
        }
        let warnings: Vec<Value> = validation
            .warnings
            .iter()
            .map(|w| {
                json!({
                    "severity": severity_to_str(w.severity),
                    "message": w.message
                })
            })
            .collect();
        let description_truncated: String = description.chars().take(120).collect();
        response["_change"] = json!({"description": description_truncated});
        if !warnings.is_empty() {
            response["warnings"] = json!(warnings);
        }
        Ok(response)
    }

    async fn handle_edit(&self, args: &Value) -> Result<Value, ToolSourceError> {
        let name = require_str(args, "name")?;
        let raw_content = require_str(args, "content")?;

        // Check skill exists before attempting edit.
        let original = match self.storage.load(name) {
            Ok(c) => c,
            Err(_) => {
                return Ok(error_response(&format!(
                    "Skill '{}' not found. Use action='create' to create a new skill.",
                    name
                )));
            }
        };

        let (parsed_name, description, triggers, body) = match validate_frontmatter(raw_content) {
            Ok(t) => t,
            Err(v) => return Ok(error_response(&format_critical_warnings(&v.warnings))),
        };

        let name_match = validate_name_match(&parsed_name, name);
        if !name_match.valid {
            return Ok(error_response(&format_critical_warnings(&name_match.warnings)));
        }

        let content = SkillContent {
            name: name.to_string(),
            description: description.clone(),
            triggers,
            lifecycle: Lifecycle::Active,
            source: Source::Auto,
            created_by: Some("agent".to_string()),
            body,
            raw: String::new(),
        };

        let validation = validate_skill_create(&content);
        if !validation.valid {
            return Ok(error_response(&format!(
                "Validation failed: {}",
                format_critical_warnings(&validation.warnings)
            )));
        }

        self.storage.save(name, &content).map_err(map_skill_err)?;

        // Security scan — roll back on block by restoring original content.
        let skill_dir = self.storage.skill_dir(Source::Auto, name);
        if let Err(report) = security::security_scan_skill(&skill_dir) {
            let _ = self.storage.save(name, &original);
            return Ok(error_response(&report));
        }

        if let Some(ref usage) = self.usage {
            usage.bump_patch(name);
        }

        let skill_md_path = skill_dir.join("SKILL.md");
        let abs_path = skill_md_path.to_string_lossy().to_string();
        let description_truncated: String = description.chars().take(120).collect();
        Ok(json!({
            "success": true,
            "message": format!("Skill '{}' updated (full rewrite).", name),
            "path": abs_path,
            "_change": {"description": description_truncated},
        }))
    }

    async fn handle_patch(&self, args: &Value) -> Result<Value, ToolSourceError> {
        let name = require_str(args, "name")?;
        let old_string = require_str(args, "old_string")?;
        let new_string = require_str(args, "new_string")?;
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let file_path = args.get("file_path").and_then(|v| v.as_str());

        if old_string.is_empty() {
            return Ok(error_response("old_string is required for 'patch'."));
        }

        // ── 1. Resolve target file + read original ──
        let snapshot = self.storage.load(name).map_err(map_skill_err)?;
        let skill_dir = self.storage.skill_dir(snapshot.source, name);

        let (target_label, target_path, original_raw) = match file_path {
            Some(fpath) => {
                let path_validation = validate_skill_path(fpath);
                if !path_validation.valid {
                    return Ok(error_response(&format!(
                        "Path validation failed: {}",
                        format_critical_warnings(&path_validation.warnings)
                    )));
                }
                let path = skill_dir.join(fpath.trim_start_matches('/'));
                let Some(content) = std::fs::read_to_string(&path).ok() else {
                    return Ok(error_response(&format!("File not found: {}", fpath)));
                };
                (fpath.to_string(), path, content)
            }
            None => {
                let path = skill_dir.join("SKILL.md");
                let raw = snapshot.raw.clone();
                ("SKILL.md".to_string(), path, raw)
            }
        };

        // ── 2. Fuzzy find-and-replace ──
        let new_content = match loom_core::text::fuzzy_replace::replace(
            &original_raw,
            old_string,
            new_string,
            replace_all,
        ) {
            Ok(result) => result,
            Err(e) => {
                return Ok(json!({
                    "success": false,
                    "error": e,
                    "file_preview": file_preview(&original_raw),
                }));
            }
        };

        let match_count = if replace_all {
            original_raw.matches(old_string).count()
        } else {
            1
        };

        // ── 3. Size check ──
        let new_bytes = new_content.len();
        if new_bytes > MAX_SKILL_FILE_BYTES {
            return Ok(error_response(&format!(
                "Patched {} would be {} bytes (limit: {} bytes / 1 MiB).",
                target_label, new_bytes, MAX_SKILL_FILE_BYTES
            )));
        }

        // ── 4. (SKILL.md only) Validate structure + content ──
        if file_path.is_none() {
            let (_, description, triggers, body) = match validate_frontmatter(&new_content) {
                Ok(t) => t,
                Err(v) => {
                    return Ok(json!({
                        "success": false,
                        "error": format!(
                            "Patch would break SKILL.md structure: {}",
                            format_critical_warnings(&v.warnings)
                        ),
                        "file_preview": file_preview(&new_content),
                    }));
                }
            };

            // Preserve existing triggers when the patch doesn't touch the triggers field.
            let triggers = if triggers.is_empty() {
                snapshot.triggers.clone()
            } else {
                triggers
            };

            let updated = SkillContent {
                name: name.to_string(),
                description: description.clone(),
                triggers,
                lifecycle: Lifecycle::Active,
                source: Source::Auto,
                created_by: Some("agent".to_string()),
                body,
                raw: new_content.clone(),
            };

            let validation = validate_skill_create(&updated);
            if !validation.valid {
                return Ok(json!({
                    "success": false,
                    "error": format!(
                        "Validation failed, patch not applied: {}",
                        format_critical_warnings(&validation.warnings)
                    ),
                    "file_preview": file_preview(&updated.raw),
                }));
            }

            self.storage.save(name, &updated).map_err(map_skill_err)?;
        } else {
            std::fs::write(&target_path, &new_content)
                .map_err(|e| map_skill_err(SkillError::Io(e)))?;
        }

        // ── 5. Security scan — roll back on block ──
        if let Err(report) = security::security_scan_skill(&skill_dir) {
            if file_path.is_none() {
                let _ = self.storage.save(name, &snapshot);
            } else {
                let _ = std::fs::write(&target_path, &original_raw);
            }
            return Ok(error_response(&report));
        }

        // ── 6. Success ──
        if let Some(ref usage) = self.usage {
            usage.bump_patch(name);
        }

        let replacement_label = if match_count == 1 { "replacement" } else { "replacements" };
        Ok(json!({
            "success": true,
            "message": format!(
                "Patched {} in skill '{}' ({} {}).",
                target_label, name, match_count, replacement_label
            ),
            "_change": {
                "old": truncate_for_change(old_string),
                "new": truncate_for_change(new_string),
            },
        }))
    }

    async fn handle_delete(&self, args: &Value) -> Result<Value, ToolSourceError> {
        let name = require_str(args, "name")?;
        let absorbed_into = args
            .get("absorbed_into")
            .and_then(|v| v.as_str())
            .map(String::from);

        if let Some(ref usage) = self.usage {
            if !usage.is_agent_created(name) {
                return Ok(error_response(&format!(
                    "Refusing to delete '{}': not agent-created. Only skills autonomously created can be deleted.",
                    name
                )));
            }
        }

        if let Some(ref target) = absorbed_into {
            if !target.is_empty() {
                if target == name {
                    return Ok(error_response(&format!(
                        "absorbed_into='{}' cannot equal the skill being deleted.",
                        target
                    )));
                }
                if self.storage.load(target).is_err() {
                    return Ok(error_response(&format!(
                        "absorbed_into='{}' does not exist.",
                        target
                    )));
                }
            }
        }

        self.storage.delete(name).map_err(map_skill_err)?;

        if let Some(ref usage) = self.usage {
            let target = absorbed_into
                .as_deref()
                .map(|s| if s.is_empty() { "" } else { s });
            usage.forget_with_intent(name, target);
        }

        let mut message = format!("Deleted skill '{}'.", name);
        if let Some(ref target) = absorbed_into {
            if !target.is_empty() {
                message.push_str(&format!(" Content absorbed into '{}'.", target));
            }
        }

        Ok(json!({
            "success": true,
            "message": message,
        }))
    }

    async fn handle_write_file(&self, args: &Value) -> Result<Value, ToolSourceError> {
        let name = require_str(args, "name")?;
        let file_path = require_str(args, "file_path")?;
        let file_content = require_str(args, "file_content")?;

        let validation = validate_skill_path(file_path);
        if !validation.valid {
            return Ok(error_response(&format!(
                "Path validation failed: {}",
                format_critical_warnings(&validation.warnings)
            )));
        }

        // Check file size limit
        let content_bytes = file_content.len();
        if content_bytes > MAX_SKILL_FILE_BYTES {
            return Ok(error_response(&format!(
                "File content is {} bytes (limit: {} bytes / 1 MiB). Consider splitting into smaller files.",
                content_bytes, MAX_SKILL_FILE_BYTES
            )));
        }

        // Check skill exists
        if self.storage.load(name).is_err() {
            return Ok(error_response(&format!(
                "Skill '{}' not found. Create it first with action='create'.",
                name
            )));
        }

        // Backup original metadata for rollback (preserves permissions on undo)
        let skill_dir = self.storage.skill_dir(Source::Auto, name);
        let target = skill_dir.join(file_path.trim_start_matches('/'));
        let original_metadata = std::fs::metadata(&target).ok();
        let original_content = std::fs::read(&target).ok();

        self.storage
            .write_file(name, file_path, file_content)
            .map_err(map_skill_err)?;

        // Security scan — roll back on block
        if let Err(report) = security::security_scan_skill(&skill_dir) {
            match &original_content {
                Some(orig) => { let _ = std::fs::write(&target, orig); }
                None => { let _ = std::fs::remove_file(&target); }
            }
            // Best-effort restore original metadata (Unix only)
            #[cfg(unix)]
            if let Some(meta) = original_metadata {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(meta.permissions().mode()));
            }
            #[cfg(not(unix))]
            { let _ = original_metadata; }
            return Ok(error_response(&report));
        }

        let abs_path = target.to_string_lossy().to_string();
        Ok(json!({
            "success": true,
            "message": format!("File '{}' written to skill '{}'.", file_path, name),
            "path": abs_path,
        }))
    }

    async fn handle_remove_file(&self, args: &Value) -> Result<Value, ToolSourceError> {
        let name = require_str(args, "name")?;
        let file_path = require_str(args, "file_path")?;

        let validation = validate_skill_path(file_path);
        if !validation.valid {
            return Ok(error_response(&format!(
                "Path validation failed: {}",
                format_critical_warnings(&validation.warnings)
            )));
        }

        if self.storage.load(name).is_err() {
            return Ok(error_response(&format!("Skill '{}' not found.", name)));
        }

        let skill_dir = self.storage.skill_dir(Source::Auto, name);
        let abs_target = skill_dir.join(file_path);
        if !abs_target.exists() {
            let available = walk_skill_files(&skill_dir);
            let mut response = json!({
                "success": false,
                "error": format!(
                    "File '{}' not found in skill '{}'.",
                    file_path, name
                ),
            });
            response["available_files"] = if available.is_empty() {
                Value::Null
            } else {
                json!(available)
            };
            return Ok(response);
        }

        self.storage
            .remove_file(name, file_path)
            .map_err(map_skill_err)?;

        Ok(json!({
            "success": true,
            "message": format!("File '{}' removed from skill '{}'.", file_path, name),
        }))
    }
}

fn walk_skill_files(skill_dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(skill_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Some(name) = p.file_name().and_then(|f| f.to_str()) {
                    out.push(name.to_string());
                }
            } else {
                for entry2 in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                    let p2 = entry2.path();
                    if p2.is_file() {
                        let dir = p.file_name().and_then(|f| f.to_str()).unwrap_or("");
                        let file = p2.file_name().and_then(|f| f.to_str()).unwrap_or("");
                        out.push(format!("{}/{}", dir, file));
                    }
                }
            }
        }
    }
    out
}

fn file_preview(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 500 {
        s.to_string()
    } else {
        let truncated: String = chars[..500].iter().collect();
        format!("{}...", truncated)
    }
}

/// Truncate a string for `_change` preview fields — mirrors Python's
/// `old_string[:200] + ("…" if len > 200 else "")`.
fn truncate_for_change(s: &str) -> String {
    let truncated: String = s.chars().take(200).collect();
    if s.chars().count() > 200 {
        format!("{}…", truncated)
    } else {
        truncated
    }
}

#[async_trait]
impl Tool for SkillManagerTool {
    fn name(&self) -> &str {
        TOOL_SKILL_MANAGE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_SKILL_MANAGE.to_string(),
            description: Some(DESCRIPTION.to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "patch", "edit", "delete", "write_file", "remove_file"],
                        "description": "The action to perform."
                    },
                    "name": {
                        "type": "string",
                        "description": "Skill name (lowercase, hyphens/underscores, max 64 chars). Must match an existing skill for patch/edit/delete/write_file/remove_file."
                    },
                    "content": {
                        "type": "string",
                        "description": "Full SKILL.md content (YAML frontmatter + markdown body). Required for 'create' and 'edit'. For 'edit', read the skill first with skill_view() and provide the complete updated text."
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Text to find in the file (required for 'patch'). Must be unique unless replace_all=true. Include enough surrounding context to ensure uniqueness."
                    },
                    "new_string": {
                        "type": "string",
                        "description": "Replacement text (required for 'patch'). Can be empty string to delete the matched text."
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "For 'patch': replace all occurrences instead of requiring a unique match (default: false)."
                    },
                    "category": {
                        "type": "string",
                        "description": "Optional category/domain for organizing the skill (e.g., 'devops', 'data-science', 'mlops'). Creates a subdirectory grouping. Only used with 'create'."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to a supporting file within the skill directory. For 'write_file'/'remove_file': required, must be under references/, templates/, scripts/, or assets/. For 'patch': optional, defaults to SKILL.md if omitted."
                    },
                    "file_content": {
                        "type": "string",
                        "description": "Content for the file. Required for 'write_file'."
                    },
                    "absorbed_into": {
                        "type": "string",
                        "description": "For 'delete' only — declares intent so the curator can tell consolidation from pruning without guessing. Pass the umbrella skill name when this skill's content was merged into another (the target must already exist). Pass an empty string when the skill is truly stale and being pruned with no forwarding target. Omitting the arg on delete is supported for backward compatibility but downstream tooling (e.g. cron-job skill reference rewriting) will have to guess at intent."
                    }
                },
                "required": ["action", "name"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let _guard = WriteOriginGuard::new(self.default_origin);

        let result = match args.get("action").and_then(|v| v.as_str()) {
            Some("create") => self.handle_create(&args).await?,
            Some("patch") => self.handle_patch(&args).await?,
            Some("edit") => self.handle_edit(&args).await?,
            Some("delete") => self.handle_delete(&args).await?,
            Some("write_file") => self.handle_write_file(&args).await?,
            Some("remove_file") => self.handle_remove_file(&args).await?,
            Some(other) => {
                return Err(ToolSourceError::InvalidInput(format!(
                    "unknown action: {}",
                    other
                )))
            }
            None => {
                return Err(ToolSourceError::InvalidInput(
                    "missing 'action' field".into(),
                ))
            }
        };

        Ok(ToolCallContent::text(result.to_string()))
    }
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolSourceError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolSourceError::InvalidInput(format!("missing or invalid '{}' field", key)))
}

fn error_response(msg: &str) -> Value {
    json!({
        "success": false,
        "error": msg,
    })
}

fn format_critical_warnings(warnings: &[ValidationWarning]) -> String {
    warnings
        .iter()
        .filter(|w| matches!(w.severity, Severity::Critical))
        .map(|w| w.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

fn severity_to_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical => "critical",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn map_skill_err(err: SkillError) -> ToolSourceError {
    ToolSourceError::ToolError(err.to_string())
}


#[cfg(test)]
mod tests;
