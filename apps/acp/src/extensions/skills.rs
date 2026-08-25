use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use super::config_store as store;
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

fn param_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn require_param(params: &Value, key: &str) -> Result<String, ExtensionError> {
    param_str(params, key)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ExtensionError::invalid_params(format!("missing required parameter: {key}")))
}

fn internal(message: impl Into<String>) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}

fn not_found(message: impl Into<String>) -> ExtensionError {
    ExtensionError::not_found(message)
}

fn mutation_envelope(message: &str) -> Value {
    store::mutation_envelope(message)
}

fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !name.starts_with('.')
}

/// Candidate skill roots in precedence order (project first).
/// Mirrors anureo's discovery chain: `.agents/skills`, `.anureo/skills`, and the
/// user-level `~/.anureo/skills` directory.
fn skill_roots(ctx: &ExtensionContext) -> Vec<(PathBuf, &'static str)> {
    let mut roots = Vec::new();
    if let Some(wd) = &ctx.working_directory {
        roots.push((wd.join(".agents").join("skills"), "project"));
        roots.push((wd.join(".anureo").join("skills"), "project"));
    }
    if let Ok(home) = store::user_home() {
        roots.push((home.join(".agents").join("skills"), "user"));
        roots.push((home.join(".anureo").join("skills"), "user"));
    }
    roots
}

fn skill_dir_in_root(root: &Path, name: &str) -> PathBuf {
    root.join(name)
}

fn find_skill(ctx: &ExtensionContext, name: &str) -> Option<(PathBuf, &'static str)> {
    for (root, scope) in skill_roots(ctx) {
        let dir = skill_dir_in_root(&root, name);
        if dir.join("SKILL.md").is_file() || dir.is_dir() {
            return Some((dir, scope));
        }
    }
    None
}

fn parse_skill_md(path: &Path) -> Result<(Map<String, Value>, String), ExtensionError> {
    store::parse_md_file(path).map_err(|e| internal(e.message))
}

fn skill_summary(name: &str, dir: &Path, scope: &str, source: &str) -> Value {
    let md = dir.join("SKILL.md");
    let (frontmatter, _) = if md.is_file() {
        parse_skill_md(&md).unwrap_or_default()
    } else {
        (Map::new(), String::new())
    };
    json!({
        "name": name,
        "scope": scope,
        "source": source,
        "path": dir.to_string_lossy(),
        "description": frontmatter.get("description").cloned().unwrap_or(Value::Null),
        "sources": {
            "md": { "exists": md.is_file(), "path": md.to_string_lossy() },
        },
    })
}

fn writable_skill_root(ctx: &ExtensionContext, scope: &str) -> Result<PathBuf, ExtensionError> {
    if scope == "project" {
        let wd = ctx
            .working_directory
            .as_deref()
            .map(Path::to_path_buf)
            .ok_or_else(|| ExtensionError::invalid_params("working directory required"))?;
        let preferred = wd.join(".agents").join("skills");
        let alternate = wd.join(".anureo").join("skills");
        if alternate.is_dir() && !preferred.is_dir() {
            return Ok(alternate);
        }
        return Ok(preferred);
    }
    let home = store::user_home().map_err(|e| internal(e.message))?;
    let preferred = home.join(".agents").join("skills");
    let alternate = home.join(".config").join("anureo").join("skills");
    if alternate.is_dir() && !preferred.is_dir() {
        return Ok(alternate);
    }
    Ok(preferred)
}

fn write_skill_dir(
    dir: &Path,
    description: &str,
    tags: Option<&Value>,
    instructions: &str,
) -> Result<(), ExtensionError> {
    std::fs::create_dir_all(dir).map_err(|e| internal(e.to_string()))?;
    let mut frontmatter = Map::new();
    frontmatter.insert("description".into(), Value::String(description.to_string()));
    if let Some(tags) = tags {
        if !tags.is_null() {
            frontmatter.insert("tags".into(), tags.clone());
        }
    }
    store::write_md_file(&dir.join("SKILL.md"), &frontmatter, instructions)
        .map_err(|e| internal(e.message))
}

// ─── Catalog ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct CatalogSourceDef {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    source: &'static str,
    source_type: &'static str,
    default_subpath: Option<&'static str>,
}

fn catalog_sources() -> Vec<CatalogSourceDef> {
    vec![
        CatalogSourceDef {
            id: "obra-superpowers",
            label: "Superpowers",
            description: "Community skill pack by obra",
            source: "https://github.com/obra/superpowers",
            source_type: "github",
            default_subpath: Some("skills"),
        },
        CatalogSourceDef {
            id: "anthropics-claude-skills",
            label: "Anthropic Skills",
            description: "Anthropic's skill examples",
            source: "https://github.com/anthropics/skills",
            source_type: "github",
            default_subpath: None,
        },
        CatalogSourceDef {
            id: "clawdhub",
            label: "ClawdHub",
            description: "ClawdHub community registry",
            source: "https://clawdhub.com",
            source_type: "clawdhub",
            default_subpath: None,
        },
    ]
}

fn clone_to_temp(url: &str) -> Result<tempfile::TempDir, ExtensionError> {
    let dir = tempfile::tempdir().map_err(|e| internal(e.to_string()))?;
    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--filter=blob:none", url])
        .arg(dir.path())
        .output()
        .map_err(|e| internal(e.to_string()))?;
    if !output.status.success() {
        return Err(internal(format!(
            "git clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(dir)
}

fn scan_skills_in_dir(
    root: &Path,
    subpath: Option<&str>,
) -> Vec<(String, Option<String>, PathBuf)> {
    let base = match subpath {
        Some(sub) if !sub.is_empty() => root.join(sub),
        _ => root.to_path_buf(),
    };
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&base) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let md = path.join("SKILL.md");
        if !md.is_file() {
            continue;
        }
        let description = parse_skill_md(&md).ok().and_then(|(fm, _)| {
            fm.get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
        out.push((name, description, path));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn catalog_item(
    source_id: &str,
    name: &str,
    description: &Option<String>,
    installed: bool,
    repo_source: &str,
    subpath: Option<&str>,
) -> Value {
    json!({
        "sourceId": source_id,
        "skillName": name,
        "frontmatterName": name,
        "description": description.clone(),
        "installable": true,
        "installed": { "isInstalled": installed },
        "repoSource": repo_source,
        "repoSubpath": subpath,
    })
}

// ─── Handler ───────────────────────────────────────────────────────────

#[derive(Default)]
pub struct SkillsHandler {
    installs: Mutex<Vec<(String, String)>>,
}

impl SkillsHandler {
    fn list(&self, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let mut skills: Vec<Value> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (root, scope) in skill_roots(ctx) {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() || seen.contains(&path) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if !valid_skill_name(&name) {
                    continue;
                }
                seen.insert(path.clone());
                skills.push(skill_summary(&name, &path, scope, "file"));
            }
        }
        skills.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        Ok(json!({ "skills": skills }))
    }

    fn get(&self, ctx: &ExtensionContext, name: &str) -> Result<Value, ExtensionError> {
        if !valid_skill_name(name) {
            return Err(ExtensionError::invalid_params("invalid skill name"));
        }
        match find_skill(ctx, name) {
            Some((dir, scope)) => {
                let md = dir.join("SKILL.md");
                let (frontmatter, instructions) = parse_skill_md(&md)?;
                Ok(json!({
                    "name": name,
                    "scope": scope,
                    "source": "file",
                    "exists": true,
                    "path": dir.to_string_lossy(),
                    "description": frontmatter.get("description").cloned().unwrap_or(Value::Null),
                    "instructions": instructions,
                    "sources": {
                        "md": { "exists": true, "path": md.to_string_lossy(), "scope": scope },
                    },
                }))
            }
            None => Ok(json!({
                "name": name,
                "scope": Value::Null,
                "source": Value::Null,
                "exists": false,
                "sources": { "md": { "exists": false } },
            })),
        }
    }

    fn create(&self, ctx: &ExtensionContext, params: &Value) -> Result<Value, ExtensionError> {
        let name = require_param(params, "name")?;
        if !valid_skill_name(&name) {
            return Err(ExtensionError::invalid_params(
                "skill name must be alphanumeric with -_. only",
            ));
        }
        let scope = param_str(params, "scope").unwrap_or_else(|| "user".into());
        let description = param_str(params, "description").unwrap_or_default();
        let instructions = param_str(params, "instructions").unwrap_or_default();
        let tags = params.get("tags").cloned();

        if find_skill(ctx, &name).is_some() {
            return Err(ExtensionError::conflict(format!(
                "skill '{name}' already exists"
            )));
        }
        let root = writable_skill_root(ctx, &scope)?;
        let dir = root.join(&name);
        write_skill_dir(&dir, &description, tags.as_ref(), &instructions)?;
        Ok(mutation_envelope(&format!(
            "Skill {name} created successfully. Reloading interface…"
        )))
    }

    fn update(&self, ctx: &ExtensionContext, params: &Value) -> Result<Value, ExtensionError> {
        let name = require_param(params, "name")?;
        let (dir, _scope) =
            find_skill(ctx, &name).ok_or_else(|| not_found(format!("skill '{name}' not found")))?;
        let md = dir.join("SKILL.md");
        let (mut frontmatter, mut instructions) = parse_skill_md(&md)?;
        if let Some(description) = param_str(params, "description") {
            frontmatter.insert("description".into(), Value::String(description));
        }
        if let Some(tags) = params.get("tags") {
            if tags.is_null() {
                frontmatter.remove("tags");
            } else {
                frontmatter.insert("tags".into(), tags.clone());
            }
        }
        if let Some(text) = param_str(params, "instructions") {
            instructions = text;
        }
        store::write_md_file(&md, &frontmatter, &instructions).map_err(|e| internal(e.message))?;
        Ok(mutation_envelope(&format!(
            "Skill {name} updated successfully. Reloading interface…"
        )))
    }

    fn delete(&self, ctx: &ExtensionContext, params: &Value) -> Result<Value, ExtensionError> {
        let name = require_param(params, "name")?;
        let (dir, _) =
            find_skill(ctx, &name).ok_or_else(|| not_found(format!("skill '{name}' not found")))?;
        std::fs::remove_dir_all(&dir).map_err(|e| internal(e.to_string()))?;
        Ok(mutation_envelope(&format!(
            "Skill {name} deleted successfully. Reloading interface…"
        )))
    }

    fn read_file(&self, ctx: &ExtensionContext, params: &Value) -> Result<Value, ExtensionError> {
        let name = require_param(params, "name")?;
        let file = require_param(params, "file")?;
        let (dir, _) =
            find_skill(ctx, &name).ok_or_else(|| not_found(format!("skill '{name}' not found")))?;
        let target = safe_join(&dir, &file)?;
        let content = std::fs::read_to_string(&target)
            .map_err(|_| not_found(format!("file '{file}' not found")))?;
        Ok(json!({ "content": content, "path": file }))
    }

    fn write_file(&self, ctx: &ExtensionContext, params: &Value) -> Result<Value, ExtensionError> {
        let name = require_param(params, "name")?;
        let file = require_param(params, "file")?;
        let content = require_param(params, "content")?;
        let (dir, _) =
            find_skill(ctx, &name).ok_or_else(|| not_found(format!("skill '{name}' not found")))?;
        let target = safe_join(&dir, &file)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| internal(e.to_string()))?;
        }
        std::fs::write(&target, content).map_err(|e| internal(e.to_string()))?;
        Ok(mutation_envelope("Skill file saved."))
    }

    fn delete_file(&self, ctx: &ExtensionContext, params: &Value) -> Result<Value, ExtensionError> {
        let name = require_param(params, "name")?;
        let file = require_param(params, "file")?;
        let (dir, _) =
            find_skill(ctx, &name).ok_or_else(|| not_found(format!("skill '{name}' not found")))?;
        let target = safe_join(&dir, &file)?;
        std::fs::remove_file(&target).map_err(|e| internal(e.to_string()))?;
        Ok(mutation_envelope("Skill file deleted."))
    }

    fn catalog_list(&self) -> Value {
        let sources: Vec<Value> = catalog_sources()
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "label": s.label,
                    "description": s.description,
                    "source": s.source,
                    "sourceType": s.source_type,
                    "defaultSubpath": s.default_subpath,
                })
            })
            .collect();
        json!({ "ok": true, "sources": sources, "itemsBySource": {}, "pageInfoBySource": {} })
    }

    fn catalog_source(
        &self,
        ctx: &ExtensionContext,
        params: &Value,
    ) -> Result<Value, ExtensionError> {
        let source_id = require_param(params, "sourceId")?;
        let def = catalog_sources()
            .into_iter()
            .find(|s| s.id == source_id)
            .ok_or_else(|| not_found(format!("unknown source: {source_id}")))?;
        if def.source_type == "clawdhub" {
            return Err(ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(
                    "clawdhub catalog requires the Express registry; pending anureo port".into(),
                )),
            });
        }
        let repo = clone_to_temp(def.source)?;
        let sub =
            param_str(params, "subpath").or_else(|| def.default_subpath.map(|s| s.to_string()));
        let items: Vec<Value> = scan_skills_in_dir(repo.path(), sub.as_deref())
            .into_iter()
            .map(|(name, description, _)| {
                let installed = find_skill(ctx, &name).is_some();
                catalog_item(
                    def.id,
                    &name,
                    &description,
                    installed,
                    def.source,
                    sub.as_deref(),
                )
            })
            .collect();
        Ok(json!({ "ok": true, "items": items }))
    }

    fn scan(&self, params: &Value) -> Result<Value, ExtensionError> {
        let source = require_param(params, "source")?;
        let subpath = param_str(params, "subpath");
        let repo = clone_to_temp(&source)?;
        let items: Vec<Value> = scan_skills_in_dir(repo.path(), subpath.as_deref())
            .into_iter()
            .map(|(name, description, _)| {
                catalog_item(
                    "custom",
                    &name,
                    &description,
                    false,
                    &source,
                    subpath.as_deref(),
                )
            })
            .collect();
        Ok(json!({ "ok": true, "items": items }))
    }

    fn install(&self, ctx: &ExtensionContext, params: &Value) -> Result<Value, ExtensionError> {
        let source = require_param(params, "source")?;
        let subpath = param_str(params, "subpath");
        let scope = param_str(params, "scope").unwrap_or_else(|| "user".into());
        let selections: Vec<String> = params
            .get("selections")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let conflict_policy =
            param_str(params, "conflictPolicy").unwrap_or_else(|| "prompt".into());
        if selections.is_empty() {
            return Err(ExtensionError::invalid_params(
                "selections must not be empty",
            ));
        }

        let repo = clone_to_temp(&source)?;
        let base = match subpath.as_deref() {
            Some(sub) if !sub.is_empty() => repo.path().join(sub),
            _ => repo.path().to_path_buf(),
        };
        let target_root = writable_skill_root(ctx, &scope)?;

        let mut installed = Vec::new();
        let mut skipped = Vec::new();
        for name in &selections {
            let src_dir = base.join(name);
            if !src_dir.join("SKILL.md").is_file() {
                skipped.push(json!({ "name": name, "reason": "not_a_skill" }));
                continue;
            }
            let dest = target_root.join(name);
            if dest.exists() {
                if conflict_policy == "skipAll" {
                    skipped.push(json!({ "name": name, "reason": "exists" }));
                    continue;
                }
                if conflict_policy == "prompt" {
                    return Err(ExtensionError::conflict(format!(
                        "skill '{name}' already exists; choose overwriteAll or skipAll"
                    )));
                }
                std::fs::remove_dir_all(&dest).map_err(|e| internal(e.to_string()))?;
            }
            copy_dir_recursive(&src_dir, &dest)?;
            installed.push(name.clone());
        }

        let mut registry = self.installs.lock().map_err(|e| internal(e.to_string()))?;
        for name in &installed {
            registry.push((name.clone(), source.clone()));
        }

        Ok(json!({
            "ok": true,
            "installed": installed,
            "skipped": skipped,
            "requiresReload": true,
            "message": format!("Installed {} skill(s).", installed.len()),
            "reloadDelayMs": 300,
        }))
    }

    fn uninstall(&self, ctx: &ExtensionContext, params: &Value) -> Result<Value, ExtensionError> {
        self.delete(ctx, params)
    }
}

fn safe_join(base: &Path, relative: &str) -> Result<PathBuf, ExtensionError> {
    let rel = Path::new(relative);
    if rel.is_absolute() || relative.contains("..") {
        return Err(ExtensionError::invalid_params("invalid file path"));
    }
    Ok(base.join(rel))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), ExtensionError> {
    std::fs::create_dir_all(dst).map_err(|e| internal(e.to_string()))?;
    for entry in std::fs::read_dir(src).map_err(|e| internal(e.to_string()))? {
        let entry = entry.map_err(|e| internal(e.to_string()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| internal(e.to_string()))?;
        }
    }
    Ok(())
}

#[async_trait::async_trait]
impl ExtensionHandler for SkillsHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => self.list(ctx),
            "get" => self.get(ctx, &require_param(&params, "name")?),
            "create" => self.create(ctx, &params),
            "update" => self.update(ctx, &params),
            "delete" => self.delete(ctx, &params),
            "uninstall" => self.uninstall(ctx, &params),
            "read_file" => self.read_file(ctx, &params),
            "write_file" => self.write_file(ctx, &params),
            "delete_file" => self.delete_file(ctx, &params),
            "catalog_list" => Ok(self.catalog_list()),
            "catalog_source" => self.catalog_source(ctx, &params),
            "scan" => self.scan(&params),
            "install" => self.install(ctx, &params),
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({
            "list": true,
            "get": true,
            "create": true,
            "update": true,
            "delete": true,
            "uninstall": true,
            "read_file": true,
            "write_file": true,
            "delete_file": true,
            "catalog_list": true,
            "catalog_source": true,
            "scan": true,
            "install": true,
        })
    }
}
