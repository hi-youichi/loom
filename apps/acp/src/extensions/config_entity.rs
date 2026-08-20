use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use super::config_store as store;
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

fn valid_snippet_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 80 {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
}

pub struct ConfigEntityHandler;

impl ConfigEntityHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConfigEntityHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn path_string(path: &std::path::Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}

fn project_agent_path(wd: &std::path::Path, name: &str) -> PathBuf {
    let plural = wd.join(store::LOOMDESK_DIR_NAME).join("agents").join(format!("{name}.md"));
    let legacy = wd.join(store::LOOMDESK_DIR_NAME).join("agent").join(format!("{name}.md"));
    if legacy.is_file() && !plural.is_file() {
        legacy
    } else {
        plural
    }
}

fn user_agent_flat_path(name: &str) -> Result<PathBuf, store::StoreError> {
    Ok(store::loomdesk_config_dir()?.join("agents").join(format!("{name}.md")))
}

fn user_agent_legacy_path(name: &str) -> Result<PathBuf, store::StoreError> {
    Ok(store::loomdesk_config_dir()?
        .parent()
        .map(|p| p.join("agent").join(format!("{name}.md")))
        .unwrap_or_else(|| PathBuf::from(format!("agent/{name}.md"))))
}

fn user_agent_path(name: &str) -> Result<PathBuf, store::StoreError> {
    let flat = user_agent_flat_path(name)?;
    if flat.is_file() {
        return Ok(flat);
    }
    let legacy = user_agent_legacy_path(name)?;
    if legacy.is_file() {
        return Ok(legacy);
    }
    if let Ok(config_dir) = store::loomdesk_config_dir() {
        let agents_dir = config_dir.join("agents");
        let index = store::index_md_files_recursive(&agents_dir);
        if let Some(found) = index.get(name) {
            return Ok(found.clone());
        }
    }
    Ok(flat)
}

fn md_sources_payload(
    md_path: Option<&std::path::Path>,
    md_scope: Option<&str>,
    body_field: &str,
) -> Result<Value, store::StoreError> {
    match md_path {
        Some(path) if path.is_file() => {
            let (frontmatter, body) = store::parse_md_file(path)?;
            let mut fields = store::fields_list(&frontmatter, None);
            if !body.is_empty() {
                fields.push(body_field.to_string());
            }
            Ok(json!({
                "exists": true,
                "path": path_string(path),
                "scope": md_scope,
                "fields": fields,
            }))
        }
        _ => Ok(json!({
            "exists": false,
            "path": md_path.map(path_string),
            "scope": Value::Null,
            "fields": [],
        })),
    }
}

fn json_sources_payload(
    source: &store::JsonEntrySource,
    layers: &store::ConfigLayers,
) -> Value {
    let path = source
        .path
        .clone()
        .or_else(|| layers.custom_path.clone())
        .or_else(|| layers.project_path.clone())
        .or_else(|| Some(layers.user_path.clone()));
    let scope = if !source.exists {
        None
    } else if source.path == layers.project_path {
        Some("project")
    } else {
        Some("user")
    };
    let fields = source
        .section
        .as_ref()
        .map(|s| s.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    json!({
        "exists": source.exists,
        "path": path.map(|p| path_string(&p)),
        "scope": scope,
        "fields": fields,
    })
}

fn entity_sources(
    ctx: &ExtensionContext,
    name: &str,
    section_key: &str,
    project_path: impl Fn(&std::path::Path) -> PathBuf,
    user_path: impl Fn(&str) -> Result<PathBuf, store::StoreError>,
    body_field: &str,
) -> Result<Value, store::StoreError> {
    let wd = store::working_dir_or_error(ctx)?;
    store::validate_entity_name(name)?;

    let project = project_path(&wd);
    let project_exists = project.is_file();
    let user = user_path(name)?;
    let user_exists = user.is_file();

    let md_path = if project_exists {
        Some(project.clone())
    } else if user_exists {
        Some(user.clone())
    } else {
        None
    };
    let md_scope = if project_exists { "project" } else { "user" };

    let layers = store::read_config_layers(Some(&wd))?;
    let json_source = store::get_json_entry_source(&layers, section_key, name);

    Ok(json!({
        "name": name,
        "sources": {
            "md": md_sources_payload(md_path.as_deref(), Some(md_scope), body_field)?,
            "json": json_sources_payload(&json_source, &layers),
            "projectMd": {
                "exists": project_exists,
                "path": path_string(&project),
            },
            "userMd": {
                "exists": user_exists,
                "path": path_string(&user),
            },
        },
        "scope": if project_exists {
            json!("project")
        } else if user_exists || json_source.exists {
            json!("user")
        } else {
            Value::Null
        },
        "isBuiltIn": !project_exists && !user_exists && !json_source.exists,
    }))
}

fn agent_config(ctx: &ExtensionContext, name: &str) -> Result<Value, store::StoreError> {
    let wd = store::working_dir_or_error(ctx)?;
    store::validate_entity_name(name)?;

    let project = project_agent_path(&wd, name);
    let project_exists = project.is_file();
    let user = user_agent_path(name)?;
    let user_exists = user.is_file();

    if project_exists || user_exists {
        let md_path = if project_exists { &project } else { &user };
        let (frontmatter, body) = store::parse_md_file(md_path)?;
        let mut config = Map::new();
        for (k, v) in frontmatter {
            config.insert(k, v);
        }
        if !body.is_empty() {
            config.insert("prompt".to_string(), Value::String(body));
        }
        return Ok(json!({
            "source": "md",
            "scope": if project_exists { "project" } else { "user" },
            "config": Value::Object(config),
        }));
    }

    let layers = store::read_config_layers(Some(&wd))?;
    let json_source = store::get_json_entry_source(&layers, store::JSON_SECTION_AGENT, name);
    if json_source.exists {
        if let Some(section) = json_source.section {
            let scope = if json_source.path == layers.project_path {
                "project"
            } else {
                "user"
            };
            return Ok(json!({
                "source": "json",
                "scope": scope,
                "config": Value::Object(section),
            }));
        }
    }

    Ok(json!({ "source": "none", "scope": Value::Null, "config": {} }))
}

type PermissionSource = (
    String,
    Option<String>,
    Option<PathBuf>,
    Option<Map<String, Value>>,
);

fn agent_permission_source(
    name: &str,
    wd: &std::path::Path,
) -> Result<PermissionSource, store::StoreError> {
    let project_md = project_agent_path(wd, name);
    if project_md.is_file() {
        let (frontmatter, _) = store::parse_md_file(&project_md)?;
        if frontmatter.contains_key("permission") {
            return Ok((
                "md".into(),
                Some("project".into()),
                Some(project_md),
                Some(frontmatter),
            ));
        }
    }

    let user_md = user_agent_path(name)?;
    if user_md.is_file() {
        let (frontmatter, _) = store::parse_md_file(&user_md)?;
        if frontmatter.contains_key("permission") {
            return Ok((
                "md".into(),
                Some("user".into()),
                Some(user_md),
                Some(frontmatter),
            ));
        }
    }

    let layers = store::read_config_layers(Some(wd))?;
    let source = store::get_json_entry_source(&layers, store::JSON_SECTION_AGENT, name);
    if let Some(section) = &source.section {
        if section.contains_key("permission") && source.path.is_some() {
            let scope = if source.path == layers.project_path {
                "project"
            } else if source.path == layers.custom_path {
                "custom"
            } else {
                "user"
            };
            return Ok(("json".into(), Some(scope.into()), source.path.clone(), source.section.clone()));
        }
    }

    Ok(("".into(), None, None, None))
}

fn merge_permission_with_non_wildcards(
    new_permission: &Value,
    existing_permission: Option<&Value>,
) -> Value {
    let Some(existing) = existing_permission else {
        return new_permission.clone();
    };
    let Some(existing_map) = existing.as_object() else {
        return new_permission.clone();
    };

    if new_permission.is_null() {
        return Value::Null;
    }
    if new_permission.is_string() {
        return new_permission.clone();
    }

    let mut non_wildcards = Map::new();
    for (perm_key, perm_value) in existing_map {
        if perm_key == "*" {
            continue;
        }
        if let Some(patterns) = perm_value.as_object() {
            let mut filtered = Map::new();
            for (pattern, action) in patterns {
                if pattern != "*" {
                    filtered.insert(pattern.clone(), action.clone());
                }
            }
            if !filtered.is_empty() {
                non_wildcards.insert(perm_key.clone(), Value::Object(filtered));
            }
        }
    }
    if non_wildcards.is_empty() {
        return new_permission.clone();
    }

    let mut merged = new_permission.as_object().cloned().unwrap_or_default();
    for (perm_key, patterns) in non_wildcards {
        match merged.get_mut(&perm_key) {
            Some(Value::String(wildcard)) => {
                let wildcard = wildcard.clone();
                let mut combined = Map::new();
                combined.insert("*".to_string(), Value::String(wildcard));
                if let Some(obj) = patterns.as_object() {
                    for (k, v) in obj {
                        combined.insert(k.clone(), v.clone());
                    }
                }
                merged.insert(perm_key, Value::Object(combined));
            }
            Some(Value::Object(current)) => {
                if let Some(obj) = patterns.as_object() {
                    for (k, v) in obj {
                        current.insert(k.clone(), v.clone());
                    }
                }
            }
            _ => {
                let existing_value = existing_map.get(&perm_key).cloned();
                match existing_value {
                    Some(Value::Object(obj)) => {
                        let mut combined = Map::new();
                        if let Some(wildcard) = obj.get("*") {
                            combined.insert("*".to_string(), wildcard.clone());
                        }
                        if let Some(pats) = patterns.as_object() {
                            for (k, v) in pats {
                                combined.insert(k.clone(), v.clone());
                            }
                        }
                        merged.insert(perm_key, Value::Object(combined));
                    }
                    _ => {
                        merged.insert(perm_key, patterns);
                    }
                }
            }
        }
    }
    Value::Object(merged)
}

#[allow(clippy::too_many_arguments)]
fn update_entity(
    ctx: &ExtensionContext,
    name: &str,
    section_key: &str,
    project_path: impl Fn(&std::path::Path) -> PathBuf,
    user_path: impl Fn(&str) -> Result<PathBuf, store::StoreError>,
    body_field: &str,
    updates: &Map<String, Value>,
    prefer_project_json: bool,
    handle_permission: bool,
) -> Result<(), store::StoreError> {
    let wd = store::working_dir_or_error(ctx)?;
    store::validate_entity_name(name)?;
    store::ensure_agent_dirs()?;

    let project = project_path(&wd);
    let project_exists = project.is_file();
    let user = user_path(name)?;
    let user_exists = user.is_file();

    let md_path = if project_exists {
        Some(project.clone())
    } else if user_exists {
        Some(user.clone())
    } else {
        None
    };
    let md_exists = md_path.is_some();

    let layers = store::read_config_layers(Some(&wd))?;
    let json_source = store::get_json_entry_source(&layers, section_key, name);
    let json_section = json_source.section.clone();
    let has_json_fields = json_source.exists
        && json_section
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);

    let (mut json_config, json_target_path) = if json_source.exists {
        (
            json_source.config.clone().unwrap_or_default(),
            json_source
                .path
                .clone()
                .unwrap_or_else(|| layers.user_path.clone()),
        )
    } else {
        store::get_json_write_target(&layers, prefer_project_json && wd.join(store::LOOMDESK_DIR_NAME).exists())
    };

    let is_builtin_override = !md_exists && !has_json_fields;

    let mut target_path = md_path.clone();
    if !md_exists && is_builtin_override {
        target_path = Some(user.clone());
    }
    let creating_new_md = is_builtin_override;

    let mut md_data = if md_exists {
        md_path
            .as_ref()
            .map(|p| store::parse_md_file(p))
            .transpose()?
    } else if creating_new_md {
        Some((Map::new(), String::new()))
    } else {
        None
    };

    let mut md_modified = false;
    let mut json_modified = false;

    for (field, value) in updates {
        if field == "scope" {
            continue;
        }

        if field == body_field {
            let normalized = match value {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };

            if value.is_null() && md_exists {
                if let Some((_, body)) = md_data.as_mut() {
                    *body = String::new();
                    md_modified = true;
                }
                continue;
            }

            if md_exists || creating_new_md {
                if let Some((_, body)) = md_data.as_mut() {
                    *body = normalized;
                    md_modified = true;
                }
                continue;
            }

            let json_body = json_section
                .as_ref()
                .and_then(|s| s.get(body_field))
                .cloned();
            if store::is_prompt_file_reference(json_body.as_ref()) {
                let reference = json_body.as_ref().and_then(|v| v.as_str()).unwrap_or_default();
                if let Some(file_path) = store::resolve_prompt_file_path(reference) {
                    store::write_prompt_file(&file_path, &normalized)?;
                }
                continue;
            }
            if store::is_prompt_file_reference(Some(&Value::String(normalized.clone()))) {
                merge_json_field(
                    &mut json_config,
                    section_key,
                    name,
                    body_field,
                    Value::String(normalized),
                );
                json_modified = true;
                continue;
            }

            merge_json_field(
                &mut json_config,
                section_key,
                name,
                body_field,
                Value::String(normalized),
            );
            json_modified = true;
            continue;
        }

        if handle_permission && field == "permission" {
            let (source_kind, _scope, source_path, source_frontmatter) =
                agent_permission_source(name, &wd)?;

            let existing_permission = if source_kind == "md" {
                source_frontmatter
                    .as_ref()
                    .and_then(|f| f.get("permission").cloned())
            } else if source_kind == "json" {
                json_section
                    .as_ref()
                    .and_then(|s| s.get("permission").cloned())
            } else {
                None
            };
            let merged = merge_permission_with_non_wildcards(value, existing_permission.as_ref());

            if source_kind == "md" {
                if let Some(path) = source_path {
                    let (mut frontmatter, body) = store::parse_md_file(&path)?;
                    frontmatter.insert("permission".into(), merged);
                    store::write_md_file(&path, &frontmatter, &body)?;
                }
            } else if source_kind == "json" {
                if let Some(path) = source_path.clone() {
                    let mut config = store::read_config_file(Some(&path))?;
                    store::section_set_entry(
                        &mut config,
                        section_key,
                        name,
                        json!({ "permission": merged }),
                    );
                    store::write_config(&config, &path)?;
                }
            } else if md_exists {
                if let Some((frontmatter, _)) = md_data.as_mut() {
                    frontmatter.insert("permission".into(), merged);
                    md_modified = true;
                }
            } else {
                merge_json_field(
                    &mut json_config,
                    section_key,
                    name,
                    "permission",
                    merged,
                );
                json_modified = true;
            }
            continue;
        }

        if value.is_null() {
            if let Some((frontmatter, _)) = md_data.as_mut() {
                if frontmatter.remove(field).is_some() {
                    md_modified = true;
                }
            }
            if has_json_fields {
                let mut entry = json_config
                    .get(section_key)
                    .and_then(|s| s.get(name).cloned())
                    .unwrap_or(Value::Null);
                let mut changed = false;
                if let Some(map) = entry.as_object_mut() {
                    changed = map.remove(field).is_some();
                    if map.is_empty() {
                        entry = Value::Null;
                    }
                }
                if entry.is_null() {
                    store::section_remove_entry(&mut json_config, section_key, name);
                } else {
                    merge_json_entry(&mut json_config, section_key, name, &entry);
                }
                if changed {
                    json_modified = true;
                }
            }
            continue;
        }

        let in_json = json_section
            .as_ref()
            .map(|s| s.contains_key(field))
            .unwrap_or(false);

        if in_json {
            merge_json_field(&mut json_config, section_key, name, field, value.clone());
            json_modified = true;
        } else if md_exists || creating_new_md {
            if let Some((frontmatter, _)) = md_data.as_mut() {
                frontmatter.insert(field.clone(), value.clone());
                md_modified = true;
            }
        } else {
            merge_json_field(&mut json_config, section_key, name, field, value.clone());
            json_modified = true;
        }
    }

    if md_modified {
        if let Some(path) = &target_path {
            if let Some((frontmatter, body)) = &md_data {
                store::write_md_file(path, frontmatter, body)?;
            }
        }
    }
    if json_modified {
        store::write_config(&json_config, &json_target_path)?;
    }

    Ok(())
}

fn merge_json_field(
    config: &mut Map<String, Value>,
    section_key: &str,
    name: &str,
    field: &str,
    value: Value,
) {
    let entry = config
        .get(section_key)
        .and_then(|s| s.get(name).cloned())
        .unwrap_or_else(|| json!({}));
    let mut map = entry.as_object().cloned().unwrap_or_default();
    map.insert(field.to_string(), value);
    merge_json_entry(config, section_key, name, &Value::Object(map));
}

fn merge_json_entry(
    config: &mut Map<String, Value>,
    section_key: &str,
    name: &str,
    entry: &Value,
) {
    let existing = config
        .get(section_key)
        .and_then(|s| s.get(name).cloned())
        .unwrap_or(Value::Null);
    let merged = store::merge_configs(&existing, entry);
    let section = config
        .entry(section_key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(map) = section {
        map.insert(name.to_string(), merged);
    }
}

#[allow(clippy::too_many_arguments)]
fn create_entity(
    ctx: &ExtensionContext,
    name: &str,
    section_key: &str,
    config: &Map<String, Value>,
    scope: Option<&str>,
    body_field: &str,
    project_path: impl Fn(&std::path::Path) -> PathBuf,
    user_path: impl Fn(&str) -> Result<PathBuf, store::StoreError>,
) -> Result<(), store::StoreError> {
    let wd = store::working_dir_or_error(ctx)?;
    store::validate_entity_name(name)?;
    store::ensure_agent_dirs()?;

    let project = project_path(&wd);
    if project.is_file() {
        return Err(store::StoreError::conflict(format!(
            "{name} already exists as project-level .md file"
        )));
    }
    let user = user_path(name)?;
    if user.is_file() {
        return Err(store::StoreError::conflict(format!(
            "{name} already exists as user-level .md file"
        )));
    }

    let layers = store::read_config_layers(Some(&wd))?;
    let json_source = store::get_json_entry_source(&layers, section_key, name);
    if json_source.exists {
        return Err(store::StoreError::conflict(format!(
            "{name} already exists in loomdesk.json"
        )));
    }

    let (target_path, _target_scope) = if scope == Some("project") {
        let dir = project.parent().unwrap_or(&wd);
        std::fs::create_dir_all(dir).map_err(|e| {
            store::StoreError::internal(format!("failed to create {}: {e}", dir.display()))
        })?;
        (project, "project")
    } else {
        (user, "user")
    };

    let mut frontmatter = Map::new();
    let mut body = String::new();
    for (key, value) in config {
        if key == "scope" || value.is_null() {
            continue;
        }
        if key == body_field {
            if let Value::String(s) = value {
                body = s.clone();
            }
            continue;
        }
        frontmatter.insert(key.clone(), value.clone());
    }

    store::write_md_file(&target_path, &frontmatter, &body)?;
    Ok(())
}

fn delete_agent(ctx: &ExtensionContext, name: &str, scope: Option<&str>) -> Result<(), store::StoreError> {
    let wd = store::working_dir_or_error(ctx)?;
    store::validate_entity_name(name)?;

    if scope.is_none() || scope == Some("project") {
        let project = project_agent_path(&wd, name);
        if project.is_file() {
            std::fs::remove_file(&project).map_err(|e| {
                store::StoreError::internal(format!("failed to delete {}: {e}", project.display()))
            })?;
            return Ok(());
        }
    }

    if scope.is_none() || scope == Some("user") {
        let user = user_agent_path(name)?;
        if user.is_file() {
            std::fs::remove_file(&user).map_err(|e| {
                store::StoreError::internal(format!("failed to delete {}: {e}", user.display()))
            })?;
            return Ok(());
        }
    }

    let layers = store::read_config_layers(Some(&wd))?;
    let source = store::get_json_entry_source(&layers, store::JSON_SECTION_AGENT, name);
    if source.exists {
        if let (Some(mut config), Some(path)) = (source.config.clone(), source.path.clone()) {
            if store::section_remove_entry(
                &mut config,
                store::JSON_SECTION_AGENT,
                name,
            ) {
                store::write_config(&config, &path)?;
                return Ok(());
            }
        }
    }

    Err(store::StoreError::not_found(format!(
        "Agent {name} is built-in or not deletable"
    )))
}

fn delete_command(ctx: &ExtensionContext, name: &str) -> Result<(), store::StoreError> {
    let wd = store::working_dir_or_error(ctx)?;
    store::validate_entity_name(name)?;

    let mut deleted = false;

    let project = wd
        .join(store::LOOMDESK_DIR_NAME)
        .join("commands")
        .join(format!("{name}.md"));
    let project_legacy = wd
        .join(store::LOOMDESK_DIR_NAME)
        .join("command")
        .join(format!("{name}.md"));
    for path in [project, project_legacy] {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| {
                store::StoreError::internal(format!("failed to delete {}: {e}", path.display()))
            })?;
            deleted = true;
        }
    }

    let config_dir = store::loomdesk_config_dir()?;
    let user_plural = config_dir.join("commands").join(format!("{name}.md"));
    let user_legacy = config_dir.join("command").join(format!("{name}.md"));
    for path in [user_plural, user_legacy] {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| {
                store::StoreError::internal(format!("failed to delete {}: {e}", path.display()))
            })?;
            deleted = true;
        }
    }

    let layers = store::read_config_layers(Some(&wd))?;
    let source = store::get_json_entry_source(&layers, store::JSON_SECTION_COMMAND, name);
    if source.exists {
        if let (Some(mut config), Some(path)) = (source.config.clone(), source.path.clone()) {
            if store::section_remove_entry(
                &mut config,
                store::JSON_SECTION_COMMAND,
                name,
            ) {
                store::write_config(&config, &path)?;
                deleted = true;
            }
        }
    }

    if !deleted {
        return Err(store::StoreError::not_found(format!(
            "Command \"{name}\" not found"
        )));
    }
    Ok(())
}

#[derive(Clone)]
struct Snippet {
    name: String,
    content: String,
    aliases: Vec<String>,
    description: Option<String>,
    file_path: PathBuf,
    source: &'static str,
}

fn snippet_dirs(ctx: &ExtensionContext) -> Result<Vec<(PathBuf, &'static str)>, store::StoreError> {
    let mut dirs = Vec::new();
    let global = store::opencode_config_dir()?;
    dirs.push((global.join("snippets"), "global"));
    dirs.push((global.join("snippet"), "global"));
    if let Some(wd) = store::optional_working_dir(ctx) {
        dirs.push((wd.join(".opencode").join("snippets"), "project"));
        dirs.push((wd.join(".opencode").join("snippet"), "project"));
    }
    Ok(dirs)
}

fn normalize_aliases(frontmatter: &Map<String, Value>) -> Vec<String> {
    let raw = frontmatter
        .get("aliases")
        .or_else(|| frontmatter.get("alias"));
    let Some(raw) = raw else {
        return Vec::new();
    };
    let list: Vec<Value> = match raw {
        Value::Array(items) => items.clone(),
        Value::Null => return Vec::new(),
        other => vec![other.clone()],
    };
    list.iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn load_snippet_registry(
    ctx: &ExtensionContext,
) -> Result<HashMap<String, Snippet>, store::StoreError> {
    let mut registry: HashMap<String, Snippet> = HashMap::new();
    let mut canonical: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (dir, source) in snippet_dirs(ctx)? {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if !file_name.ends_with(".md") {
                continue;
            }
            let name = file_name.trim_end_matches(".md").to_string();
            if !valid_snippet_name(&name) {
                continue;
            }
            let path = entry.path();
            let Ok((frontmatter, body)) = store::parse_md_file(&path) else {
                continue;
            };
            let aliases = normalize_aliases(&frontmatter);
            let description = frontmatter
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let snippet = Snippet {
                name: name.clone(),
                content: body,
                aliases: aliases.clone(),
                description,
                file_path: path,
                source,
            };

            let key = name.to_lowercase();
            let stale_aliases: Vec<String> = match registry.get(&key) {
                Some(existing)
                    if existing.name.to_lowercase() == key && !canonical.contains(&key) =>
                {
                    existing.aliases.clone()
                }
                _ => Vec::new(),
            };
            for alias in stale_aliases {
                let alias_key = alias.to_lowercase();
                if !canonical.contains(&alias_key) {
                    registry.remove(&alias_key);
                }
            }
            canonical.insert(key.clone());
            registry.insert(key.clone(), snippet);
            for alias in aliases {
                let alias_key = alias.to_lowercase();
                if valid_snippet_name(&alias_key) && !canonical.contains(&alias_key) {
                    if let Some(snippet) = registry.get(&key).cloned() {
                        registry.insert(alias_key, snippet);
                    }
                }
            }
        }
    }
    Ok(registry)
}

fn list_unique_snippets(registry: &HashMap<String, Snippet>) -> Vec<&Snippet> {
    let mut seen = std::collections::HashSet::new();
    let mut snippets: Vec<&Snippet> = Vec::new();
    for snippet in registry.values() {
        let key = format!("{}:{}", snippet.source, snippet.file_path.display());
        if seen.insert(key) {
            snippets.push(snippet);
        }
    }
    snippets.sort_by(|a, b| a.name.cmp(&b.name));
    snippets
}

fn snippet_json(snippet: &Snippet) -> Value {
    json!({
        "name": snippet.name,
        "content": snippet.content,
        "aliases": snippet.aliases,
        "description": snippet.description,
        "filePath": snippet.file_path.to_string_lossy(),
        "source": snippet.source,
    })
}

fn writable_snippet_dir(
    ctx: &ExtensionContext,
    scope: &str,
) -> Result<PathBuf, store::StoreError> {
    if scope == "project" {
        let wd = store::working_dir_or_error(ctx)?;
        let preferred = wd.join(".opencode").join("snippet");
        let alternate = wd.join(".opencode").join("snippets");
        if alternate.is_dir() && !preferred.is_dir() {
            return Ok(alternate);
        }
        return Ok(preferred);
    }
    let global = store::opencode_config_dir()?;
    let alt = global.join("snippets");
    let preferred = global.join("snippet");
    if alt.is_dir() && !preferred.is_dir() {
        Ok(alt)
    } else {
        Ok(preferred)
    }
}

fn write_snippet_file(path: &std::path::Path, content: &str, aliases: &[String], description: Option<&str>) -> Result<(), store::StoreError> {
    let mut frontmatter = Map::new();
    let normalized: Vec<String> = aliases
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    if !normalized.is_empty() {
        frontmatter.insert(
            "aliases".into(),
            Value::Array(normalized.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(desc) = description {
        if !desc.trim().is_empty() {
            frontmatter.insert("description".into(), Value::String(desc.trim().to_string()));
        }
    }

    let mut output = String::new();
    if !frontmatter.is_empty() {
        let yaml_str = serde_yaml::to_string(&{
            let mut m = serde_yaml::Mapping::new();
            for (k, v) in &frontmatter {
                m.insert(
                    serde_yaml::Value::String(k.clone()),
                    json_value_to_yaml(v),
                );
            }
            serde_yaml::Value::Mapping(m)
        })
        .map_err(|e| store::StoreError::internal(format!("failed to serialize snippet: {e}")))?;
        output.push_str("---\n");
        output.push_str(&yaml_str);
        output.push_str("---\n");
        if !content.is_empty() {
            output.push('\n');
        }
    }
    output.push_str(content);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| store::StoreError::internal(format!("failed to create directory: {e}")))?;
    }
    std::fs::write(path, output)
        .map_err(|e| store::StoreError::internal(format!("failed to write snippet: {e}")))?;
    Ok(())
}

fn json_value_to_yaml(value: &Value) -> serde_yaml::Value {
    match value {
        Value::Null => serde_yaml::Value::Null,
        Value::Bool(b) => serde_yaml::Value::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_yaml::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_yaml::Value::Number(u.into())
            } else {
                serde_yaml::Value::Number(n.as_f64().unwrap_or(0.0).into())
            }
        }
        Value::String(s) => serde_yaml::Value::String(s.clone()),
        Value::Array(items) => serde_yaml::Value::Sequence(
            items.iter().map(json_value_to_yaml).collect(),
        ),
        Value::Object(map) => {
            let mut out = serde_yaml::Mapping::new();
            for (k, v) in map {
                out.insert(
                    serde_yaml::Value::String(k.clone()),
                    json_value_to_yaml(v),
                );
            }
            serde_yaml::Value::Mapping(out)
        }
    }
}

fn find_snippet<'a>(
    registry: &'a HashMap<String, Snippet>,
    name: &str,
) -> Option<&'a Snippet> {
    registry.get(&name.to_lowercase())
}

#[derive(Default)]
struct ExpansionCollector {
    prepend: Vec<String>,
    append: Vec<String>,
}

fn parse_snippet_blocks(content: &str) -> (String, Vec<String>, Vec<String>) {
    let mut prepend = Vec::new();
    let mut append = Vec::new();
    let mut inline = content.to_string();

    for block_type in ["prepend", "append"] {
        let open = format!("<{block_type}>");
        let close = format!("</{block_type}>");
        let mut remaining = inline.clone();
        let mut output = String::new();
        loop {
            let Some(start) = remaining.find(&open) else {
                output.push_str(&remaining);
                break;
            };
            output.push_str(&remaining[..start]);
            let after = remaining[start + open.len()..].to_string();
            let end = after.find(&close).unwrap_or(after.len());
            let value = after[..end].trim();
            if !value.is_empty() {
                match block_type {
                    "prepend" => prepend.push(value.to_string()),
                    _ => append.push(value.to_string()),
                }
            }
            remaining = after[end.min(after.len())..].to_string();
            if let Some(rest) = remaining.strip_prefix(&close) {
                remaining = rest.to_string();
            }
        }
        inline = output;
    }

    let open = "<inject>";
    let close = "</inject>";
    let mut remaining = inline;
    let mut output = String::new();
    loop {
        let Some(start) = remaining.find(open) else {
            output.push_str(remaining.as_str());
            break;
        };
        output.push_str(&remaining[..start]);
        let after = remaining[start + open.len()..].to_string();
        let end = after.find(close).unwrap_or(after.len());
        remaining = after[end.min(after.len())..].to_string();
        if let Some(rest) = remaining.strip_prefix(close) {
            remaining = rest.to_string();
        }
    }

    (output.trim().to_string(), prepend, append)
}

fn expand_text(
    text: &str,
    registry: &HashMap<String, Snippet>,
    expansion_counts: &mut HashMap<String, u32>,
    collector: &mut ExpansionCollector,
) -> String {
    let mut current = text.to_string();
    loop {
        let mut result = String::new();
        let mut last = 0;
        let mut matched = false;
        let mut loop_detected = false;

        let chars: Vec<char> = current.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '#' && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
                let mut j = i + 1;
                let mut name = String::new();
                while j < chars.len()
                    && (chars[j].is_ascii_alphanumeric()
                        || chars[j] == '_'
                        || chars[j] == '-')
                {
                    name.push(chars[j]);
                    j += 1;
                }
                if !name.is_empty() {
                    let is_skill_call = name.eq_ignore_ascii_case("skill")
                        && j < chars.len()
                        && chars[j] == '(';
                    if !is_skill_call {
                        if let Some(snippet) = registry.get(&name.to_lowercase()) {
                            let key = snippet.name.to_lowercase();
                            let count = expansion_counts.get(&key).copied().unwrap_or(0) + 1;
                            if count > 15 {
                                loop_detected = true;
                            } else {
                                expansion_counts.insert(key, count);
                                let (inline, prepend_blocks, append_blocks) =
                                    parse_snippet_blocks(&snippet.content);
                                for block in &prepend_blocks {
                                    let expanded = expand_text(
                                        block,
                                        registry,
                                        expansion_counts,
                                        collector,
                                    );
                                    collector.prepend.push(expanded);
                                }
                                for block in &append_blocks {
                                    let expanded = expand_text(
                                        block,
                                        registry,
                                        expansion_counts,
                                        collector,
                                    );
                                    collector.append.push(expanded);
                                }
                                let expanded_inline = expand_text(
                                    &inline,
                                    registry,
                                    expansion_counts,
                                    collector,
                                );
                                result.push_str(&current[last..i]);
                                result.push_str(&expanded_inline);
                                last = j;
                                matched = true;
                            }
                        }
                    }
                }
                i = j;
                continue;
            }
            i += 1;
        }

        result.push_str(&current[last.min(current.len())..]);
        current = result;
        if !matched || loop_detected {
            break;
        }
    }
    current
}

fn expand_snippets(text: &str, ctx: &ExtensionContext) -> Result<String, store::StoreError> {
    let registry = load_snippet_registry(ctx)?;
    let mut collector = ExpansionCollector::default();
    let mut counts = HashMap::new();
    let expanded = expand_text(text, &registry, &mut counts, &mut collector)
        .trim()
        .to_string();
    let mut parts = collector.prepend;
    parts.push(expanded);
    parts.extend(collector.append);
    Ok(parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n"))
}

fn params_object(params: &Value) -> Map<String, Value> {
    params.as_object().cloned().unwrap_or_default()
}

#[async_trait::async_trait]
impl ExtensionHandler for ConfigEntityHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let result = match method {
            "agents_sources" | "commands_sources" => {
                let name = store::params_str_required(&params, "name")?;
                if method == "agents_sources" {
                    entity_sources(ctx, &name, store::JSON_SECTION_AGENT, |wd| project_agent_path(wd, &name), user_agent_path, "prompt")
                } else {
                    entity_sources(
                        ctx,
                        &name,
                        store::JSON_SECTION_COMMAND,
                        |wd| {
                            let plural = wd.join(store::LOOMDESK_DIR_NAME).join("commands").join(format!("{name}.md"));
                            let legacy = wd.join(store::LOOMDESK_DIR_NAME).join("command").join(format!("{name}.md"));
                            if legacy.is_file() && !plural.is_file() { legacy } else { plural }
                        },
                        |n| {
                            let config_dir = store::loomdesk_config_dir()?;
                            let plural = config_dir.join("commands").join(format!("{n}.md"));
                            let legacy = config_dir.join("command").join(format!("{n}.md"));
                            if legacy.is_file() && !plural.is_file() { Ok(legacy) } else { Ok(plural) }
                        },
                        "template",
                    )
                }
            }
            "agents_config" => {
                let name = store::params_str_required(&params, "name")?;
                agent_config(ctx, &name)
            }
            "agents_create" => {
                let name = store::params_str_required(&params, "name")?;
                let scope = store::scope_from_params(&params);
                create_entity(
                    ctx,
                    &name,
                    store::JSON_SECTION_AGENT,
                    &params_object(&params),
                    scope.as_deref(),
                    "prompt",
                    |wd| project_agent_path(wd, &name),
                    user_agent_path,
                )?;
                Ok(store::mutation_envelope(&format!(
                    "Agent {name} created successfully. Reloading interface…"
                )))
            }
            "commands_create" => {
                let name = store::params_str_required(&params, "name")?;
                let scope = store::scope_from_params(&params);
                create_entity(
                    ctx,
                    &name,
                    store::JSON_SECTION_COMMAND,
                    &params_object(&params),
                    scope.as_deref(),
                    "template",
                    |wd| {
                        let plural = wd.join(store::LOOMDESK_DIR_NAME).join("commands").join(format!("{name}.md"));
                        let legacy = wd.join(store::LOOMDESK_DIR_NAME).join("command").join(format!("{name}.md"));
                        if legacy.is_file() && !plural.is_file() { legacy } else { plural }
                    },
                    |n| {
                        let config_dir = store::loomdesk_config_dir()?;
                        let plural = config_dir.join("commands").join(format!("{n}.md"));
                        let legacy = config_dir.join("command").join(format!("{n}.md"));
                        if legacy.is_file() && !plural.is_file() { Ok(legacy) } else { Ok(plural) }
                    },
                )?;
                Ok(store::mutation_envelope(&format!(
                    "Command {name} created successfully. Reloading interface…"
                )))
            }
            "agents_update" => {
                let name = store::params_str_required(&params, "name")?;
                let updates = params_object(&params);
                update_entity(
                    ctx,
                    &name,
                    store::JSON_SECTION_AGENT,
                    |wd| project_agent_path(wd, &name),
                    user_agent_path,
                    "prompt",
                    &updates,
                    false,
                    true,
                )?;
                Ok(store::mutation_envelope(&format!(
                    "Agent {name} updated successfully. Reloading interface…"
                )))
            }
            "commands_update" => {
                let name = store::params_str_required(&params, "name")?;
                let updates = params_object(&params);
                update_entity(
                    ctx,
                    &name,
                    store::JSON_SECTION_COMMAND,
                    |wd| {
                        let plural = wd.join(store::LOOMDESK_DIR_NAME).join("commands").join(format!("{name}.md"));
                        let legacy = wd.join(store::LOOMDESK_DIR_NAME).join("command").join(format!("{name}.md"));
                        if legacy.is_file() && !plural.is_file() { legacy } else { plural }
                    },
                    |n| {
                        let config_dir = store::loomdesk_config_dir()?;
                        let plural = config_dir.join("commands").join(format!("{n}.md"));
                        let legacy = config_dir.join("command").join(format!("{n}.md"));
                        if legacy.is_file() && !plural.is_file() { Ok(legacy) } else { Ok(plural) }
                    },
                    "template",
                    &updates,
                    true,
                    false,
                )?;
                Ok(store::mutation_envelope(&format!(
                    "Command {name} updated successfully. Reloading interface…"
                )))
            }
            "agents_delete" => {
                let name = store::params_str_required(&params, "name")?;
                let scope = store::scope_from_params(&params);
                delete_agent(ctx, &name, scope.as_deref())?;
                Ok(store::mutation_envelope(&format!(
                    "Agent {name} deleted successfully. Reloading interface…"
                )))
            }
            "commands_delete" => {
                let name = store::params_str_required(&params, "name")?;
                delete_command(ctx, &name)?;
                Ok(store::mutation_envelope(&format!(
                    "Command {name} deleted successfully. Reloading interface…"
                )))
            }
            "snippets_list" => {
                let registry = load_snippet_registry(ctx)?;
                Ok(Value::Array(
                    list_unique_snippets(&registry)
                        .into_iter()
                        .map(snippet_json)
                        .collect(),
                ))
            }
            "snippets_get" => {
                let name = store::params_str_required(&params, "name")?;
                if !valid_snippet_name(&name) {
                    return Err(ExtensionError::from(store::StoreError::invalid(
                        "Snippet name must use letters, numbers, dashes, or underscores",
                    )));
                }
                let registry = load_snippet_registry(ctx)?;
                match find_snippet(&registry, &name) {
                    Some(snippet) => Ok(snippet_json(snippet)),
                    None => Err(store::StoreError::not_found(format!(
                        "Snippet \"{name}\" not found"
                    ))),
                }
            }
            "snippets_create" => {
                let name = store::params_str_required(&params, "name")?;
                if !valid_snippet_name(&name) {
                    return Err(ExtensionError::from(store::StoreError::invalid(
                        "Snippet name must use letters, numbers, dashes, or underscores",
                    )));
                }
                let scope = store::params_str(&params, "scope").unwrap_or_else(|| "global".into());
                let dir = writable_snippet_dir(ctx, &scope)?;
                let path = dir.join(format!("{name}.md"));
                if path.exists() {
                    return Err(ExtensionError::from(store::StoreError::conflict(format!(
                        "Snippet \"{name}\" already exists"
                    ))));
                }
                let content = store::params_str(&params, "content").unwrap_or_default();
                let aliases = params
                    .get("aliases")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let description = store::params_str(&params, "description");
                write_snippet_file(&path, &content, &aliases, description.as_deref())?;
                let registry = load_snippet_registry(ctx)?;
                match find_snippet(&registry, &name) {
                    Some(snippet) => Ok(json!({ "success": true, "snippet": snippet_json(snippet) })),
                    None => Err(store::StoreError::internal("snippet vanished after write")),
                }
            }
            "snippets_update" => {
                let name = store::params_str_required(&params, "name")?;
                if !valid_snippet_name(&name) {
                    return Err(ExtensionError::from(store::StoreError::invalid(
                        "Snippet name must use letters, numbers, dashes, or underscores",
                    )));
                }
                let registry = load_snippet_registry(ctx)?;
                let Some(existing) = find_snippet(&registry, &name) else {
                    return Err(ExtensionError::from(store::StoreError::not_found(format!(
                        "Snippet \"{name}\" not found"
                    ))));
                };
                let content = store::params_str(&params, "content")
                    .unwrap_or_else(|| existing.content.clone());
                let aliases = match params.get("aliases") {
                    Some(Value::Array(items)) => items
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect(),
                    _ => existing.aliases.clone(),
                };
                let description = match params.get("description") {
                    Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.clone()),
                    _ => existing.description.clone(),
                };
                write_snippet_file(
                    &existing.file_path,
                    &content,
                    &aliases,
                    description.as_deref(),
                )?;
                let registry = load_snippet_registry(ctx)?;
                match find_snippet(&registry, &name) {
                    Some(snippet) => Ok(json!({ "success": true, "snippet": snippet_json(snippet) })),
                    None => Err(store::StoreError::internal("snippet vanished after write")),
                }
            }
            "snippets_delete" => {
                let name = store::params_str_required(&params, "name")?;
                if !valid_snippet_name(&name) {
                    return Err(ExtensionError::from(store::StoreError::invalid(
                        "Snippet name must use letters, numbers, dashes, or underscores",
                    )));
                }
                let registry = load_snippet_registry(ctx)?;
                let Some(existing) = find_snippet(&registry, &name) else {
                    return Err(ExtensionError::from(store::StoreError::not_found(format!(
                        "Snippet \"{name}\" not found"
                    ))));
                };
                std::fs::remove_file(&existing.file_path).map_err(|e| {
                    store::StoreError::internal(format!("failed to delete snippet: {e}"))
                })?;
                Ok(json!({ "success": true }))
            }
            "snippets_expand" => {
                let text = store::params_str(&params, "text").unwrap_or_default();
                let expanded = expand_snippets(&text, ctx)?;
                Ok(json!({ "text": expanded }))
            }
            _ => return Err(ExtensionError::method_not_found()),
        };
        result.map_err(ExtensionError::from)
    }

    fn capabilities(&self) -> Value {
        json!({
            "agents_sources": true,
            "agents_config": true,
            "agents_create": true,
            "agents_update": true,
            "agents_delete": true,
            "commands_sources": true,
            "commands_create": true,
            "commands_update": true,
            "commands_delete": true,
            "snippets_list": true,
            "snippets_get": true,
            "snippets_create": true,
            "snippets_update": true,
            "snippets_delete": true,
            "snippets_expand": true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    fn make_ctx(dir: &std::path::Path) -> ExtensionContext {
        ExtensionContext {
            session_id: Some("test-session".into()),
            principal: "test-user".into(),
            connection_id: "test-conn".into(),
            working_directory: Some(dir.to_path_buf()),
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    struct HomeGuard {
        old: Option<String>,
    }

    impl HomeGuard {
        fn set(home: &std::path::Path) -> Self {
            let old = std::env::var("LOOMDESK_TEST_USER_HOME").ok();
            std::env::set_var("LOOMDESK_TEST_USER_HOME", home);
            Self { old }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(v) => std::env::set_var("LOOMDESK_TEST_USER_HOME", v),
                None => std::env::remove_var("LOOMDESK_TEST_USER_HOME"),
            }
        }
    }

    fn setup() -> (TempDir, TempDir, HomeGuard) {
        let project = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let guard = HomeGuard::set(home.path());
        (project, home, guard)
    }

    fn write_snippet(dir: &std::path::Path, name: &str, body: &str, frontmatter: &str) {
        fs::create_dir_all(dir).unwrap();
        let mut content = String::new();
        if !frontmatter.is_empty() {
            content.push_str("---\n");
            content.push_str(frontmatter);
            content.push_str("\n---\n\n");
        }
        content.push_str(body);
        fs::write(dir.join(format!("{name}.md")), content).unwrap();
    }

    async fn call(
        handler: &ConfigEntityHandler,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        handler.handle(method, params, ctx).await
    }

    #[test]
    fn snippet_names_are_validated() {
        assert!(valid_snippet_name("hello"));
        assert!(valid_snippet_name("a-b_c9"));
        assert!(!valid_snippet_name(""));
        assert!(!valid_snippet_name("-lead"));
        assert!(!valid_snippet_name("has space"));
        assert!(!valid_snippet_name(".dot"));
        assert!(!valid_snippet_name(&"x".repeat(81)));
    }

    #[test]
    fn strip_jsonc_removes_comments_and_trailing_commas() {
        let src = r#"{
  // line comment
  "a": "b", /* block */
  "list": [1, 2, 3,],
  "url": "http://not-a-comment",
}"#;
        let parsed: Value = serde_json::from_str(&store::strip_jsonc(src)).unwrap();
        assert_eq!(parsed["a"], "b");
        assert_eq!(parsed["url"], "http://not-a-comment");
        assert_eq!(parsed["list"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn md_file_roundtrips_frontmatter_and_body() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.md");
        let mut fm = Map::new();
        fm.insert("description".into(), Value::String("d".into()));
        fm.insert("temperature".into(), Value::from(0.7));
        fm.insert("disabled".into(), Value::Bool(true));
        store::write_md_file(&path, &fm, "hello body").unwrap();
        let (parsed, body) = store::parse_md_file(&path).unwrap();
        assert_eq!(parsed["description"], "d");
        assert_eq!(parsed["temperature"], 0.7);
        assert_eq!(parsed["disabled"], true);
        assert_eq!(body, "hello body");
    }

    #[tokio::test]
    #[serial]
    async fn agent_crud_via_md_files() {
        let (project, home, _guard) = setup();
        let ctx = make_ctx(project.path());
        let handler = ConfigEntityHandler::new();

        call(
            &handler,
            "agents_create",
            serde_json::json!({ "name": "reviewer", "prompt": "you review", "description": "d" }),
            &ctx,
        )
        .await
        .unwrap();

        let user_md = home
            .path()
            .join(".config")
            .join("loomdesk")
            .join("agents")
            .join("reviewer.md");
        assert!(user_md.is_file());
        let (fm, body) = store::parse_md_file(&user_md).unwrap();
        assert_eq!(fm["description"], "d");
        assert_eq!(body, "you review");

        let sources = call(&handler, "agents_sources", serde_json::json!({ "name": "reviewer" }), &ctx)
            .await
            .unwrap();
        assert_eq!(sources["sources"]["md"]["exists"], true);
        assert_eq!(sources["scope"], "user");
        assert_eq!(sources["isBuiltIn"], false);

        let config = call(&handler, "agents_config", serde_json::json!({ "name": "reviewer" }), &ctx)
            .await
            .unwrap();
        assert_eq!(config["source"], "md");
        assert_eq!(config["config"]["prompt"], "you review");

        call(
            &handler,
            "agents_update",
            serde_json::json!({ "name": "reviewer", "prompt": "new prompt" }),
            &ctx,
        )
        .await
        .unwrap();
        let (_, body) = store::parse_md_file(&user_md).unwrap();
        assert_eq!(body, "new prompt");

        let conflict = call(
            &handler,
            "agents_create",
            serde_json::json!({ "name": "reviewer", "prompt": "x" }),
            &ctx,
        )
        .await;
        assert!(conflict.is_err());

        call(&handler, "agents_delete", serde_json::json!({ "name": "reviewer" }), &ctx)
            .await
            .unwrap();
        assert!(!user_md.exists());
    }

    #[tokio::test]
    #[serial]
    async fn agent_json_layer_and_delete() {
        let (project, home, _guard) = setup();
        let ctx = make_ctx(project.path());
        let handler = ConfigEntityHandler::new();

        let user_config = home.path().join(".config").join("loomdesk").join("config.json");
        fs::create_dir_all(user_config.parent().unwrap()).unwrap();
        fs::write(
            &user_config,
            serde_json::json!({ "agent": { "builtin-override": { "description": "from json" } } }).to_string(),
        )
        .unwrap();

        let sources = call(
            &handler,
            "agents_sources",
            serde_json::json!({ "name": "builtin-override" }),
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(sources["sources"]["json"]["exists"], true);
        assert_eq!(sources["scope"], "user");

        call(
            &handler,
            "agents_delete",
            serde_json::json!({ "name": "builtin-override" }),
            &ctx,
        )
        .await
        .unwrap();
        let after: Value =
            serde_json::from_str(&fs::read_to_string(&user_config).unwrap()).unwrap();
        assert!(after.get("agent").is_none());
    }

    #[tokio::test]
    #[serial]
    async fn command_crud_prefers_project_json_target() {
        let (project, home, _guard) = setup();
        let ctx = make_ctx(project.path());
        let handler = ConfigEntityHandler::new();

        call(
            &handler,
            "commands_create",
            serde_json::json!({ "name": "ship", "template": "deploy it", "description": "d" }),
            &ctx,
        )
        .await
        .unwrap();

        let user_md = home
            .path()
            .join(".config")
            .join("loomdesk")
            .join("commands")
            .join("ship.md");
        assert!(user_md.is_file());

        call(
            &handler,
            "commands_update",
            serde_json::json!({ "name": "ship", "template": "deploy v2" }),
            &ctx,
        )
        .await
        .unwrap();
        let (_, body) = store::parse_md_file(&user_md).unwrap();
        assert_eq!(body, "deploy v2");

        call(&handler, "commands_delete", serde_json::json!({ "name": "ship" }), &ctx)
            .await
            .unwrap();
        assert!(!user_md.exists());
    }

    #[tokio::test]
    #[serial]
    async fn snippet_crud_and_registry() {
        let (project, _home, _guard) = setup();
        let ctx = make_ctx(project.path());
        let handler = ConfigEntityHandler::new();

        let created = call(
            &handler,
            "snippets_create",
            serde_json::json!({
                "name": "greet",
                "content": "hello world",
                "aliases": ["hi", "yo"],
                "description": "greeting"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(created["success"], true);
        assert_eq!(created["snippet"]["name"], "greet");

        let list = call(&handler, "snippets_list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);

        let by_alias = call(&handler, "snippets_get", serde_json::json!({ "name": "hi" }), &ctx)
            .await
            .unwrap();
        assert_eq!(by_alias["name"], "greet");

        call(
            &handler,
            "snippets_update",
            serde_json::json!({ "name": "greet", "content": "hi there" }),
            &ctx,
        )
        .await
        .unwrap();
        let got = call(&handler, "snippets_get", serde_json::json!({ "name": "greet" }), &ctx)
            .await
            .unwrap();
        assert_eq!(got["content"], "hi there");

        call(&handler, "snippets_delete", serde_json::json!({ "name": "greet" }), &ctx)
            .await
            .unwrap();
        let list = call(&handler, "snippets_list", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(list.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    #[serial]
    async fn snippet_expand_resolves_hashtags_and_blocks() {
        let (project, home, _guard) = setup();
        let ctx = make_ctx(project.path());
        let handler = ConfigEntityHandler::new();

        let global = home.path().join(".config").join("opencode").join("snippets");
        write_snippet(&global, "greet", "hello from greet", "");
        write_snippet(
            &global,
            "wrap",
            "<prepend>before-block</prepend>\nwrapped says #greet\n<append>after-block</append>",
            "",
        );
        write_snippet(&global, "loopy-a", "a says #loopy-b", "");
        write_snippet(&global, "loopy-b", "b says #loopy-a", "");

        let result = call(
            &handler,
            "snippets_expand",
            serde_json::json!({ "text": "start #wrap end" }),
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(
            result["text"],
            "before-block\n\nstart wrapped says hello from greet end\n\nafter-block"
        );

        let skill = call(
            &handler,
            "snippets_expand",
            serde_json::json!({ "text": "call #skill(frontend-review) now" }),
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(skill["text"], "call #skill(frontend-review) now");

        let looped = call(
            &handler,
            "snippets_expand",
            serde_json::json!({ "text": "#loopy-a" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!looped["text"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn project_snippet_overrides_global() {
        let (project, home, _guard) = setup();
        let ctx = make_ctx(project.path());
        let handler = ConfigEntityHandler::new();

        let global = home.path().join(".config").join("opencode").join("snippets");
        write_snippet(&global, "dup", "global version", "");

        let project_snippets = project.path().join(".opencode").join("snippets");
        write_snippet(&project_snippets, "dup", "project version", "");

        let got = call(&handler, "snippets_get", serde_json::json!({ "name": "dup" }), &ctx)
            .await
            .unwrap();
        assert_eq!(got["content"], "project version");
    }
}
