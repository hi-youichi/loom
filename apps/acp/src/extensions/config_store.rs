use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{ExtensionContext, ExtensionError};

pub const LOOMDESK_DIR_NAME: &str = ".loomdesk";
pub const AGENT_DIR_NAME: &str = "agents";
pub const COMMAND_DIR_NAME: &str = "commands";
pub const JSON_SECTION_AGENT: &str = "agent";
pub const JSON_SECTION_COMMAND: &str = "command";
pub const PROMPT_FILE_PATTERN_PREFIX: &str = "{file:";
pub const CLIENT_RELOAD_DELAY_MS: u64 = 300;

#[derive(Debug)]
pub struct StoreError {
    pub message: String,
    pub status: u16,
}

impl StoreError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: 500,
        }
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: 404,
        }
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: 409,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: 400,
        }
    }
}

impl From<StoreError> for ExtensionError {
    fn from(err: StoreError) -> Self {
        match err.status {
            404 => ExtensionError::not_found(err.message),
            409 => ExtensionError::conflict(err.message),
            400 => ExtensionError::invalid_params(err.message),
            _ => ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String(err.message)),
            },
        }
    }
}

pub fn user_home() -> Result<PathBuf, StoreError> {
    if let Ok(test_home) = std::env::var("LOOMDESK_TEST_USER_HOME") {
        if !test_home.trim().is_empty() {
            return Ok(PathBuf::from(test_home));
        }
    }
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var(key)
        .map(PathBuf::from)
        .map_err(|_| StoreError::internal("unable to resolve user home directory"))
}

pub fn loomdesk_config_dir() -> Result<PathBuf, StoreError> {
    Ok(user_home()?.join(".config").join("loomdesk"))
}

pub fn loom_config_dir() -> Result<PathBuf, StoreError> {
    Ok(user_home()?.join(".config").join("loom"))
}

pub fn validate_entity_name(name: &str) -> Result<(), StoreError> {
    if name.is_empty()
        || name.len() > 255
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.starts_with('.')
    {
        return Err(StoreError::invalid(format!("invalid entity name: {name}")));
    }
    Ok(())
}

pub fn parse_md_file(path: &Path) -> Result<(Map<String, Value>, String), StoreError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| StoreError::internal(format!("failed to read {}: {e}", path.display())))?;

    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let yaml_src = &rest[..end];
            let mut body = &rest[end + 4..];
            if let Some(stripped) = body.strip_prefix('\n') {
                body = stripped;
            } else if let Some(stripped) = body.strip_prefix("\r\n") {
                body = stripped;
            }
            let frontmatter = if yaml_src.trim().is_empty() {
                Map::new()
            } else {
                let value: serde_yaml::Value = serde_yaml::from_str(yaml_src).map_err(|e| {
                    StoreError::internal(format!(
                        "failed to parse frontmatter in {}: {e}",
                        path.display()
                    ))
                })?;
                yaml_to_json_object(value).unwrap_or_default()
            };
            return Ok((frontmatter, body.trim().to_string()));
        }
    }

    if let Some(rest) = content.strip_prefix("---\r\n") {
        if let Some(end) = rest.find("\r\n---") {
            let yaml_src = &rest[..end];
            let mut body = &rest[end + 5..];
            if let Some(stripped) = body.strip_prefix("\r\n") {
                body = stripped;
            }
            let frontmatter = if yaml_src.trim().is_empty() {
                Map::new()
            } else {
                let value: serde_yaml::Value = serde_yaml::from_str(yaml_src).map_err(|e| {
                    StoreError::internal(format!(
                        "failed to parse frontmatter in {}: {e}",
                        path.display()
                    ))
                })?;
                yaml_to_json_object(value).unwrap_or_default()
            };
            return Ok((frontmatter, body.trim().to_string()));
        }
    }

    Ok((Map::new(), content.trim().to_string()))
}

fn yaml_to_json(value: serde_yaml::Value) -> Value {
    match value {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => Value::Bool(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(u) = n.as_u64() {
                Value::from(u)
            } else {
                Value::from(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_yaml::Value::String(s) => Value::String(s),
        serde_yaml::Value::Sequence(seq) => {
            Value::Array(seq.into_iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let key = match k {
                    serde_yaml::Value::String(s) => s,
                    serde_yaml::Value::Number(n) => n.to_string(),
                    serde_yaml::Value::Bool(b) => b.to_string(),
                    _ => continue,
                };
                out.insert(key, yaml_to_json(v));
            }
            Value::Object(out)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

fn yaml_to_json_object(value: serde_yaml::Value) -> Option<Map<String, Value>> {
    match yaml_to_json(value) {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

fn json_to_yaml(value: &Value) -> serde_yaml::Value {
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
        Value::Array(items) => {
            serde_yaml::Value::Sequence(items.iter().map(json_to_yaml).collect())
        }
        Value::Object(map) => {
            let mut out = serde_yaml::Mapping::new();
            for (k, v) in map {
                out.insert(serde_yaml::Value::String(k.clone()), json_to_yaml(v));
            }
            serde_yaml::Value::Mapping(out)
        }
    }
}

pub fn write_md_file(
    path: &Path,
    frontmatter: &Map<String, Value>,
    body: &str,
) -> Result<(), StoreError> {
    let mut cleaned = serde_yaml::Mapping::new();
    for (key, value) in frontmatter {
        if value.is_null() {
            continue;
        }
        cleaned.insert(serde_yaml::Value::String(key.clone()), json_to_yaml(value));
    }

    let mut content = String::new();
    if !cleaned.is_empty() {
        let yaml_str = serde_yaml::to_string(&serde_yaml::Value::Mapping(cleaned))
            .map_err(|e| StoreError::internal(format!("failed to serialize frontmatter: {e}")))?;
        content.push_str("---\n");
        content.push_str(&yaml_str);
        content.push_str("---\n\n");
    }
    content.push_str(body);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            StoreError::internal(format!("failed to create {}: {e}", parent.display()))
        })?;
    }
    std::fs::write(path, content)
        .map_err(|e| StoreError::internal(format!("failed to write {}: {e}", path.display())))?;
    Ok(())
}

pub fn strip_jsonc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut string_escaped = false;

    while i < bytes.len() {
        let ch = bytes[i];
        let next = bytes.get(i + 1).copied();

        if in_string {
            out.push(ch);
            if string_escaped {
                string_escaped = false;
            } else if ch == '\\' {
                string_escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
                i += 1;
            }
            '/' if next == Some('/') => {
                while i < bytes.len() && bytes[i] != '\n' {
                    i += 1;
                }
            }
            '/' if next == Some('*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            _ => {
                out.push(ch);
                i += 1;
            }
        }
    }

    strip_trailing_commas(&out)
}

fn strip_trailing_commas(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == ']' || chars[j] == '}') {
                i += 1;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

pub fn read_config_file(path: Option<&Path>) -> Result<Map<String, Value>, StoreError> {
    let Some(path) = path else {
        return Ok(Map::new());
    };
    if !path.exists() {
        return Ok(Map::new());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| StoreError::internal(format!("failed to read {}: {e}", path.display())))?;
    if content.trim().is_empty() {
        return Ok(Map::new());
    }
    let normalized = strip_jsonc(&content);
    let value: Value = serde_json::from_str(&normalized)
        .map_err(|e| StoreError::internal(format!("failed to parse {}: {e}", path.display())))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

pub struct ConfigLayers {
    pub user_config: Map<String, Value>,
    pub project_config: Map<String, Value>,
    pub custom_config: Map<String, Value>,
    pub user_path: PathBuf,
    pub project_path: Option<PathBuf>,
    pub custom_path: Option<PathBuf>,
}

pub fn project_config_candidates(working_dir: &Path) -> Vec<PathBuf> {
    vec![
        working_dir.join("loomdesk.json"),
        working_dir.join("loomdesk.jsonc"),
        working_dir.join(LOOMDESK_DIR_NAME).join("loomdesk.json"),
        working_dir.join(LOOMDESK_DIR_NAME).join("loomdesk.jsonc"),
    ]
}

fn project_config_path(working_dir: Option<&Path>) -> Option<PathBuf> {
    let wd = working_dir?;
    let candidates = project_config_candidates(wd);
    candidates
        .iter()
        .find(|c| c.exists())
        .cloned()
        .or_else(|| candidates.first().cloned())
}

pub fn custom_config_path() -> Option<PathBuf> {
    std::env::var("LOOM_CONFIG").ok().map(PathBuf::from)
}

pub fn read_config_layers(working_dir: Option<&Path>) -> Result<ConfigLayers, StoreError> {
    let config_dir = loomdesk_config_dir()?;
    let user_candidates = [
        config_dir.join("config.json"),
        config_dir.join("loomdesk.json"),
        config_dir.join("loomdesk.jsonc"),
    ];
    let user_path = user_candidates
        .iter()
        .find(|c| c.exists())
        .cloned()
        .unwrap_or_else(|| user_candidates[0].clone());
    let project_path = project_config_path(working_dir);
    let custom_path = custom_config_path();

    Ok(ConfigLayers {
        user_config: read_config_file(Some(&user_path))?,
        project_config: read_config_file(project_path.as_deref())?,
        custom_config: read_config_file(custom_path.as_deref())?,
        user_path,
        project_path,
        custom_path,
    })
}

pub fn merge_configs(base: &Value, override_value: &Value) -> Value {
    match (base, override_value) {
        (Value::Object(b), Value::Object(o)) => {
            let mut result = b.clone();
            for (key, value) in o {
                let merged = match result.get(key) {
                    Some(existing) => merge_configs(existing, value),
                    None => value.clone(),
                };
                result.insert(key.clone(), merged);
            }
            Value::Object(result)
        }
        _ => override_value.clone(),
    }
}

pub fn write_config(config: &Map<String, Value>, path: &Path) -> Result<(), StoreError> {
    if path.exists() {
        let backup = path.with_extension("loomdesk.backup");
        std::fs::copy(path, &backup)
            .map_err(|e| StoreError::internal(format!("failed to back up config: {e}")))?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            StoreError::internal(format!("failed to create {}: {e}", parent.display()))
        })?;
    }
    let json = serde_json::to_string_pretty(&Value::Object(config.clone()))
        .map_err(|e| StoreError::internal(format!("failed to serialize config: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| StoreError::internal(format!("failed to write {}: {e}", path.display())))?;
    Ok(())
}

pub struct JsonEntrySource {
    pub section: Option<Map<String, Value>>,
    pub config: Option<Map<String, Value>>,
    pub path: Option<PathBuf>,
    pub exists: bool,
}

pub fn get_json_entry_source(
    layers: &ConfigLayers,
    section_key: &str,
    entry_name: &str,
) -> JsonEntrySource {
    let lookup = |config: &Map<String, Value>| -> Option<Map<String, Value>> {
        config
            .get(section_key)?
            .get(entry_name)?
            .as_object()
            .cloned()
    };

    if let Some(section) = lookup(&layers.custom_config) {
        return JsonEntrySource {
            section: Some(section),
            config: Some(layers.custom_config.clone()),
            path: layers.custom_path.clone(),
            exists: true,
        };
    }
    if let Some(section) = lookup(&layers.project_config) {
        return JsonEntrySource {
            section: Some(section),
            config: Some(layers.project_config.clone()),
            path: layers.project_path.clone(),
            exists: true,
        };
    }
    if let Some(section) = lookup(&layers.user_config) {
        return JsonEntrySource {
            section: Some(section),
            config: Some(layers.user_config.clone()),
            path: Some(layers.user_path.clone()),
            exists: true,
        };
    }
    JsonEntrySource {
        section: None,
        config: None,
        path: None,
        exists: false,
    }
}

pub fn get_json_write_target(
    layers: &ConfigLayers,
    prefer_project: bool,
) -> (Map<String, Value>, PathBuf) {
    if let Some(custom_path) = &layers.custom_path {
        return (layers.custom_config.clone(), custom_path.clone());
    }
    if prefer_project {
        if let Some(project_path) = &layers.project_path {
            return (layers.project_config.clone(), project_path.clone());
        }
    }
    (layers.user_config.clone(), layers.user_path.clone())
}

pub fn section_set_entry(
    config: &mut Map<String, Value>,
    section_key: &str,
    entry_name: &str,
    entry: Value,
) {
    let section = config
        .entry(section_key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(map) = section {
        map.insert(entry_name.to_string(), entry);
    }
}

pub fn section_remove_entry(
    config: &mut Map<String, Value>,
    section_key: &str,
    entry_name: &str,
) -> bool {
    let Some(section) = config.get_mut(section_key) else {
        return false;
    };
    let Some(map) = section.as_object_mut() else {
        return false;
    };
    if map.remove(entry_name).is_none() {
        return false;
    }
    if map.is_empty() {
        config.remove(section_key);
    }
    true
}

pub fn is_prompt_file_reference(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            trimmed.len() > PROMPT_FILE_PATTERN_PREFIX.len() + 1
                && trimmed.starts_with(PROMPT_FILE_PATTERN_PREFIX)
                && trimmed.ends_with('}')
        }
        _ => false,
    }
}

pub fn resolve_prompt_file_path(reference: &str) -> Option<PathBuf> {
    let trimmed = reference.trim();
    let inner = trimmed
        .strip_prefix(PROMPT_FILE_PATTERN_PREFIX)?
        .strip_suffix('}')?;
    let target = inner.trim();
    if target.is_empty() {
        return None;
    }
    let config_dir = loomdesk_config_dir().ok()?;
    if let Some(rel) = target.strip_prefix("./") {
        return Some(config_dir.join(rel));
    }
    let path = PathBuf::from(target);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(config_dir.join(path))
    }
}

pub fn write_prompt_file(path: &Path, content: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            StoreError::internal(format!("failed to create {}: {e}", parent.display()))
        })?;
    }
    std::fs::write(path, content)
        .map_err(|e| StoreError::internal(format!("failed to write {}: {e}", path.display())))?;
    Ok(())
}

pub fn mutation_envelope(message: &str) -> Value {
    serde_json::json!({
        "success": true,
        "requiresReload": true,
        "message": message,
        "reloadDelayMs": CLIENT_RELOAD_DELAY_MS,
    })
}

pub fn working_dir_or_error(ctx: &ExtensionContext) -> Result<PathBuf, StoreError> {
    ctx.working_directory
        .as_deref()
        .map(Path::to_path_buf)
        .ok_or_else(|| StoreError::invalid("working directory is required for this operation"))
}

pub fn optional_working_dir(ctx: &ExtensionContext) -> Option<PathBuf> {
    ctx.working_directory.as_deref().map(Path::to_path_buf)
}

pub fn params_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn params_str_required(params: &Value, key: &str) -> Result<String, StoreError> {
    params_str(params, key)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| StoreError::invalid(format!("missing required parameter: {key}")))
}

pub fn scope_from_params(params: &Value) -> Option<String> {
    params_str(params, "scope").filter(|s| s == "user" || s == "project")
}

pub fn fields_list(frontmatter: &Map<String, Value>, extra_field: Option<&str>) -> Vec<String> {
    let mut fields: Vec<String> = frontmatter.keys().cloned().collect();
    if let Some(extra) = extra_field {
        fields.push(extra.to_string());
    }
    fields
}

pub type Frontmatter = Map<String, Value>;

pub fn ensure_agent_dirs() -> Result<(), StoreError> {
    let config_dir = loomdesk_config_dir()?;
    for dir in [
        config_dir.clone(),
        config_dir.join(AGENT_DIR_NAME),
        config_dir.join(COMMAND_DIR_NAME),
    ] {
        std::fs::create_dir_all(&dir).map_err(|e| {
            StoreError::internal(format!("failed to create {}: {e}", dir.display()))
        })?;
    }
    Ok(())
}

pub fn index_md_files_recursive(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut index = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<_> = entries.flatten().collect();
        names.sort_by_key(|e| e.file_name());
        for entry in names {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if entry.file_name().to_string_lossy().ends_with(".md") {
                let stem = entry
                    .file_name()
                    .to_string_lossy()
                    .trim_end_matches(".md")
                    .to_string();
                index.entry(stem).or_insert(path);
            }
        }
    }
    index
}

pub fn md_file_exists(path: &Path) -> bool {
    path.is_file()
}
