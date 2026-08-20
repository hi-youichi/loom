use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};

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
        .ok_or_else(|| {
            ExtensionError::invalid_params(format!("missing required parameter: {key}"))
        })
}

fn internal(message: impl Into<String>) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}

fn mutation_envelope(message: &str) -> Value {
    store::mutation_envelope(message)
}

fn parse_spec_kind(spec: &str) -> &'static str {
    let trimmed = spec.trim();
    if trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with("~/")
        || (trimmed.len() > 2 && trimmed.as_bytes()[1] == b':' && trimmed.as_bytes()[2] == b'\\')
    {
        "path"
    } else {
        "npm"
    }
}

fn split_spec_version(spec: &str) -> (String, Option<String>) {
    // Handle @scope/name@version while keeping the leading @scope.
    let trimmed = spec.trim();
    if let Some(rest) = trimmed.strip_prefix('@') {
        if let Some(idx) = rest.rfind('@') {
            if idx > 0 {
                return (
                    format!("@{}", &rest[..idx]),
                    Some(rest[idx + 1..].to_string()),
                );
            }
        }
        (trimmed.to_string(), None)
    } else if let Some(idx) = trimmed.rfind('@') {
        (
            trimmed[..idx].to_string(),
            Some(trimmed[idx + 1..].to_string()),
        )
    } else {
        (trimmed.to_string(), None)
    }
}

fn entry_id(scope: &str, spec: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("config:{scope}:{spec}"))
}

fn file_id(scope: &str, file_name: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("file:{scope}:{file_name}"))
}

const PLUGIN_FILE_RE: &str = r"^[a-z0-9][a-z0-9-_.]*\.(js|ts|mjs|cjs)$";

fn valid_plugin_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if !lower.contains('.') {
        return false;
    }
    let head_ok = lower
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let body_ok = lower.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.')
    });
    let ext_ok = [".js", ".ts", ".mjs", ".cjs"]
        .iter()
        .any(|ext| lower.ends_with(ext));
    head_ok && body_ok && ext_ok && !name.contains("..")
}

fn plugin_file_dir(scope: &str, ctx: &ExtensionContext) -> Result<PathBuf, ExtensionError> {
    if scope == "project" {
        let wd = ctx
            .working_directory
            .as_deref()
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| ExtensionError::invalid_params("working directory required"))?;
        return Ok(wd.join(store::LOOMDESK_DIR_NAME).join("plugins"));
    }
    Ok(store::loomdesk_config_dir()
        .map_err(|e| internal(e.message))?
        .join("plugins"))
}

fn plugin_entries(layers: &store::ConfigLayers) -> Vec<Value> {
    for config in [
        &layers.custom_config,
        &layers.project_config,
        &layers.user_config,
    ] {
        if let Some(Value::Array(items)) = config.get("plugin") {
            return items.clone();
        }
    }
    Vec::new()
}

#[derive(Default)]
struct RegistryCacheEntry {
    fetched_at: Option<Instant>,
    body: Option<Value>,
}

#[derive(Default)]
pub struct PluginHandler {
    registry_cache: Mutex<std::collections::HashMap<String, RegistryCacheEntry>>,
    in_flight: Mutex<std::collections::HashMap<String, Value>>,
}

impl PluginHandler {
    fn list(&self, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let wd = store::working_dir_or_error(ctx).map_err(|e| internal(e.message))?;
        let layers = store::read_config_layers(Some(&wd)).map_err(|e| internal(e.message))?;
        let entries = plugin_entries(&layers);
        let sources = [
            layers.custom_path.clone(),
            layers.project_path.clone(),
            Some(layers.user_path.clone()),
        ];
        let source_path = entries_path(&sources);

        let mut out = Vec::new();
        for item in &entries {
            let (spec, options) = match item {
                Value::String(spec) => (spec.clone(), None),
                Value::Array(pair) if pair.len() == 2 => {
                    let spec = pair[0].as_str().unwrap_or_default().to_string();
                    let options = pair[1].as_object().cloned();
                    (spec, options)
                }
                _ => continue,
            };
            let scope = scope_of_entry(item, &layers);
            out.push(json!({
                "id": entry_id(&scope, &spec),
                "spec": spec,
                "options": options,
                "scope": scope,
                "kind": "config",
                "parsedKind": parse_spec_kind(&spec),
                "sourcePath": source_path,
            }));
        }

        let mut files = Vec::new();
        for scope in ["user", "project"] {
            let Ok(dir) = plugin_file_dir(scope, ctx) else {
                continue;
            };
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !valid_plugin_file_name(&name) {
                        continue;
                    }
                    files.push(json!({
                        "id": file_id(scope, &name),
                        "fileName": name,
                        "scope": scope,
                        "kind": "file",
                        "absolutePath": entry.path().to_string_lossy(),
                    }));
                }
            }
        }
        files.sort_by(|a, b| a["fileName"].as_str().cmp(&b["fileName"].as_str()));

        Ok(json!({ "entries": out, "files": files }))
    }

    fn entry_get(&self, ctx: &ExtensionContext, id: &str) -> Result<Value, ExtensionError> {
        let items = self.list(ctx)?;
        items["entries"]
            .as_array()
            .and_then(|arr| arr.iter().find(|e| e["id"].as_str() == Some(id)))
            .cloned()
            .ok_or_else(|| ExtensionError::not_found(format!("plugin entry not found: {id}")))
    }

    fn entry_create(
        &self,
        ctx: &ExtensionContext,
        params: &Value,
    ) -> Result<Value, ExtensionError> {
        let spec = require_param(params, "spec")?;
        let scope = param_str(params, "scope").unwrap_or_else(|| "user".into());
        let options = params.get("options").and_then(|v| v.as_object()).cloned();

        if parse_spec_kind(&spec) == "path" {
            let path = PathBuf::from(&spec);
            if !path.exists() {
                return Err(ExtensionError::invalid_params(format!(
                    "plugin path does not exist: {spec}"
                )));
            }
        }

        let wd = store::working_dir_or_error(ctx).map_err(|e| internal(e.message))?;
        let layers = store::read_config_layers(Some(&wd)).map_err(|e| internal(e.message))?;
        let mut existing = plugin_entries(&layers);
        if existing
            .iter()
            .any(|e| matches!(e, Value::String(s) if *s == spec)
                || matches!(e, Value::Array(p) if !p.is_empty() && p[0].as_str() == Some(&spec)))
        {
            return Err(ExtensionError::conflict(format!(
                "plugin already configured: {spec}"
            )));
        }
        existing.push(match &options {
            None => Value::String(spec.clone()),
            Some(opts) => json!([spec, opts]),
        });

        let (mut config, path) = store::get_json_write_target(
            &layers,
            scope == "project" && wd.join(store::LOOMDESK_DIR_NAME).exists(),
        );
        config.insert("plugin".into(), Value::Array(existing));
        store::write_config(&config, &path).map_err(|e| internal(e.message))?;
        Ok(mutation_envelope(&format!(
            "Plugin {spec} added successfully. Reloading interface…"
        )))
    }

    fn entry_update(
        &self,
        ctx: &ExtensionContext,
        params: &Value,
    ) -> Result<Value, ExtensionError> {
        let id = require_param(params, "id")?;
        let current = self.entry_get(ctx, &id)?;
        let old_spec = current["spec"].as_str().unwrap_or_default().to_string();
        let scope = current["scope"].as_str().unwrap_or("user").to_string();
        let spec = param_str(params, "spec").unwrap_or_else(|| old_spec.clone());
        let options = match params.get("options") {
            Some(Value::Object(map)) => Some(map.clone()),
            Some(Value::Null) => None,
            _ => current["options"].as_object().cloned(),
        };

        let wd = store::working_dir_or_error(ctx).map_err(|e| internal(e.message))?;
        let layers = store::read_config_layers(Some(&wd)).map_err(|e| internal(e.message))?;
        let mut entries = plugin_entries(&layers);
        let target_idx = entries.iter().position(|e| {
            matches!(e, Value::String(s) if *s == old_spec)
                || matches!(e, Value::Array(p) if !p.is_empty() && p[0].as_str() == Some(&old_spec))
        });
        let Some(idx) = target_idx else {
            return Err(ExtensionError::not_found("plugin entry vanished"));
        };
        entries[idx] = match &options {
            None => Value::String(spec.clone()),
            Some(opts) => json!([spec, opts]),
        };

        let path = current["sourcePath"].as_str().map(PathBuf::from);
        let path = path
            .filter(|p| p.exists())
            .unwrap_or_else(|| layers.user_path.clone());
        let mut config = store::read_config_file(Some(&path)).map_err(|e| internal(e.message))?;
        config.insert("plugin".into(), Value::Array(entries));
        store::write_config(&config, &path).map_err(|e| internal(e.message))?;
        let _ = scope;
        Ok(mutation_envelope(&format!(
            "Plugin {spec} updated successfully. Reloading interface…"
        )))
    }

    fn entry_delete(
        &self,
        ctx: &ExtensionContext,
        params: &Value,
    ) -> Result<Value, ExtensionError> {
        let id = require_param(params, "id")?;
        let current = self.entry_get(ctx, &id)?;
        let spec = current["spec"].as_str().unwrap_or_default().to_string();

        let wd = store::working_dir_or_error(ctx).map_err(|e| internal(e.message))?;
        let layers = store::read_config_layers(Some(&wd)).map_err(|e| internal(e.message))?;
        let entries = plugin_entries(&layers);
        let kept: Vec<Value> = entries
            .into_iter()
            .filter(|e| {
                !(matches!(e, Value::String(s) if *s == spec)
                    || matches!(e, Value::Array(p) if !p.is_empty() && p[0].as_str() == Some(&spec)))
            })
            .collect();

        let path = current["sourcePath"].as_str().map(PathBuf::from);
        let path = path
            .filter(|p| p.exists())
            .unwrap_or_else(|| layers.user_path.clone());
        let mut config = store::read_config_file(Some(&path)).map_err(|e| internal(e.message))?;
        if kept.is_empty() {
            config.remove("plugin");
        } else {
            config.insert("plugin".into(), Value::Array(kept));
        }
        store::write_config(&config, &path).map_err(|e| internal(e.message))?;
        Ok(mutation_envelope(&format!(
            "Plugin {spec} removed successfully. Reloading interface…"
        )))
    }

    fn file_read(&self, ctx: &ExtensionContext, id: &str) -> Result<Value, ExtensionError> {
        let listing = self.list(ctx)?;
        let file = listing["files"]
            .as_array()
            .and_then(|arr| arr.iter().find(|f| f["id"].as_str() == Some(id)))
            .cloned()
            .ok_or_else(|| ExtensionError::not_found("plugin file not found"))?;
        let path = file["absolutePath"].as_str().unwrap_or_default();
        let content = std::fs::read_to_string(path)
            .map_err(|_| ExtensionError::not_found("plugin file read failed"))?;
        Ok(json!({
            "fileName": file["fileName"],
            "scope": file["scope"],
            "content": content,
        }))
    }

    fn file_write(
        &self,
        ctx: &ExtensionContext,
        params: &Value,
        create: bool,
    ) -> Result<Value, ExtensionError> {
        let file_name = require_param(params, "fileName")?;
        if !valid_plugin_file_name(&file_name) {
            return Err(ExtensionError::invalid_params(format!(
                "invalid plugin file name (expected {PLUGIN_FILE_RE})"
            )));
        }
        let content = param_str(params, "content").unwrap_or_default();
        let scope = param_str(params, "scope").unwrap_or_else(|| "user".into());
        let dir = plugin_file_dir(&scope, ctx)?;
        let path = dir.join(&file_name);
        if create && path.exists() {
            return Err(ExtensionError::conflict(format!(
                "plugin file already exists: {file_name}"
            )));
        }
        if !create && !path.exists() {
            return Err(ExtensionError::not_found("plugin file not found"));
        }
        std::fs::create_dir_all(&dir).map_err(|e| internal(e.to_string()))?;
        std::fs::write(&path, content).map_err(|e| internal(e.to_string()))?;
        Ok(mutation_envelope(&format!(
            "Plugin file {file_name} saved. Reloading interface…"
        )))
    }

    fn file_delete(&self, ctx: &ExtensionContext, id: &str) -> Result<Value, ExtensionError> {
        let listing = self.list(ctx)?;
        let file = listing["files"]
            .as_array()
            .and_then(|arr| arr.iter().find(|f| f["id"].as_str() == Some(id)))
            .cloned()
            .ok_or_else(|| ExtensionError::not_found("plugin file not found"))?;
        let path = file["absolutePath"].as_str().unwrap_or_default();
        std::fs::remove_file(path).map_err(|e| internal(e.to_string()))?;
        Ok(mutation_envelope("Plugin file deleted."))
    }

    async fn registry(
        &self,
        params: &Value,
    ) -> Result<Value, ExtensionError> {
        let specs: Vec<String> = params
            .get("specs")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if specs.is_empty() {
            return Err(ExtensionError::invalid_params("specs must not be empty"));
        }

        let mut results = Vec::new();
        for spec in specs {
            if parse_spec_kind(&spec) == "path" {
                let path = PathBuf::from(&spec);
                results.push(json!({
                    "kind": "path",
                    "spec": spec,
                    "exists": path.exists(),
                }));
                continue;
            }
            let (name, version) = split_spec_version(&spec);
            match self.fetch_npm_metadata(&name, version).await {
                Ok(meta) => results.push(meta),
                Err(message) => results.push(json!({
                    "kind": "npm",
                    "spec": spec,
                    "error": message,
                })),
            }
        }
        Ok(json!({ "results": results }))
    }

    async fn fetch_npm_metadata(
        &self,
        name: &str,
        version: Option<String>,
    ) -> Result<Value, String> {
        let cache_key = format!("{name}@{}", version.as_deref().unwrap_or("latest"));
        {
            let cache = self.registry_cache.lock().map_err(|e| e.to_string())?;
            if let Some(entry) = cache.get(&cache_key) {
                if let (Some(at), Some(body)) = (entry.fetched_at, entry.body.as_ref()) {
                    if at.elapsed() < Duration::from_secs(3600) {
                        return Ok(body.clone());
                    }
                }
            }
            if let Some(body) = self
                .in_flight
                .lock()
                .ok()
                .and_then(|map| map.get(&cache_key).cloned())
            {
                return Ok(body);
            }
        }

        let url = format!("https://registry.npmjs.org/{}", name);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        let response = client.get(&url).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("npm registry returned {}", response.status()));
        }
        let body: Value = response.json().await.map_err(|e| e.to_string())?;
        let latest = body
            .get("dist-tags")
            .and_then(|t| t.get("latest"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let target_version = version.clone().or_else(|| latest.clone());
        let current_version = body
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let versions: Vec<String> = body
            .get("versions")
            .and_then(|v| v.as_object())
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        let result = json!({
            "kind": "npm",
            "spec": format!("{name}@{}", target_version.as_deref().unwrap_or("")),
            "name": name,
            "currentVersion": current_version,
            "latestVersion": latest,
            "versions": if versions.len() > 50 { versions[versions.len()-50..].to_vec() } else { versions },
        });

        if let Ok(mut cache) = self.registry_cache.lock() {
            cache.insert(
                cache_key.clone(),
                RegistryCacheEntry {
                    fetched_at: Some(Instant::now()),
                    body: Some(result.clone()),
                },
            );
        }
        Ok(result)
    }
}

fn entries_path(sources: &[Option<PathBuf>]) -> Value {
    for source in sources.iter().flatten() {
        if source.exists() {
            return Value::String(source.to_string_lossy().into_owned());
        }
    }
    Value::Null
}

fn scope_of_entry(entry: &Value, layers: &store::ConfigLayers) -> String {
    // Heuristic: entries are stored in a single layer file per config; we
    // cannot tell per-entry scope from the array alone, so report the layer
    // that carries the `plugin` section.
    let _ = entry;
    if layers.custom_path.is_some() {
        "custom".into()
    } else if layers.project_path.is_some() && layers.project_config.contains_key("plugin") {
        "project".into()
    } else {
        "user".into()
    }
}

#[async_trait::async_trait]
impl ExtensionHandler for PluginHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => self.list(ctx),
            "entry_get" => {
                let id = require_param(&params, "id")?;
                self.entry_get(ctx, &id)
            }
            "entry_create" => self.entry_create(ctx, &params),
            "entry_update" => self.entry_update(ctx, &params),
            "entry_delete" => {
                self.entry_delete(ctx, &params)
            }
            "file_read" => {
                let id = require_param(&params, "id")?;
                self.file_read(ctx, &id)
            }
            "file_create" => self.file_write(ctx, &params, true),
            "file_update" => self.file_write(ctx, &params, false),
            "file_delete" => {
                let id = require_param(&params, "id")?;
                self.file_delete(ctx, &id)
            }
            "registry" => self.registry(&params).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({
            "list": true,
            "entry_get": true,
            "entry_create": true,
            "entry_update": true,
            "entry_delete": true,
            "file_read": true,
            "file_create": true,
            "file_update": true,
            "file_delete": true,
            "registry": true,
        })
    }
}
