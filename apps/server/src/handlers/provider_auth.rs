//! Provider auth CRUD endpoints (OC-compat T4).
//!
//! Implements 4 OpenChamber-compatible endpoints for managing provider API keys
//! in `~/.loom/config.toml`:
//! - `GET /provider/auth` — list auth methods per provider
//! - `POST /provider/:providerId/auth` — write API key to config.toml
//! - `GET /provider/:providerId/source` — check if provider has API key
//! - `DELETE /provider/:providerId/auth` — remove API key from config.toml

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::state::SharedState;

const CONFIG_APP_NAME: &str = "loom";

/// `GET /provider/auth` — Returns auth methods for each configured provider.
///
/// All Loom providers use API Key auth. Returns:
/// ```json
/// { "provider-id": [{ "type": "api", "label": "Manually enter API Key" }] }
/// ```
pub async fn get_provider_auth(
    State(_state): State<SharedState>,
) -> Json<Value> {
    let cfg = match config::load_full_config(CONFIG_APP_NAME) {
        Ok(c) => c,
        Err(_) => return Json(json!({})),
    };

    let mut result = serde_json::Map::new();
    for def in &cfg.providers {
        result.insert(def.name.clone(), json!([
            { "type": "api", "label": "Manually enter API Key" }
        ]));
    }

    Json(Value::Object(result))
}

/// `POST /provider/:providerId/auth` — Write API key to config.toml.
///
/// Body: `{ "apikey": "sk-xxx" }`
/// Reads the current config.toml as raw text, updates the matching provider's
/// `api_key` field, and writes it back atomically (temp file + rename).
pub async fn post_provider_auth(
    State(_state): State<SharedState>,
    Path(provider_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let apikey = body
        .get("apikey")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if apikey.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "apikey is required" })),
        ));
    }

    match update_provider_in_config(&provider_id, |toml_content| {
        set_provider_field(toml_content, &provider_id, "api_key", apikey)
    }) {
        Ok(()) => Ok(Json(json!({ "success": true }))),
        Err(msg) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": msg })),
        )),
    }
}

/// `GET /provider/:providerId/source` — Check provider connection status.
///
/// Returns:
/// ```json
/// { "providerId": "...", "sources": { "auth": { "exists": true/false } } }
/// ```
pub async fn get_provider_source(
    State(_state): State<SharedState>,
    Path(provider_id): Path<String>,
) -> Json<Value> {
    let cfg = config::load_full_config(CONFIG_APP_NAME).unwrap_or_default();
    let exists = cfg
        .providers
        .iter()
        .find(|p| p.name == provider_id)
        .and_then(|p| p.api_key.as_ref())
        .is_some();

    Json(json!({
        "providerId": provider_id,
        "sources": {
            "auth": { "exists": exists }
        }
    }))
}

/// `DELETE /provider/:providerId/auth` — Remove API key from config.toml.
pub async fn delete_provider_auth(
    State(_state): State<SharedState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match update_provider_in_config(&provider_id, |toml_content| {
        remove_provider_field(toml_content, &provider_id, "api_key")
    }) {
        Ok(()) => Ok(Json(json!({
            "success": true,
            "message": "Provider disconnected"
        }))),
        Err(msg) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": msg })),
        )),
    }
}

/// Read config.toml as raw text, apply a transformation, write back atomically.
fn update_provider_in_config<F>(provider_id: &str, transform: F) -> Result<(), String>
where
    F: FnOnce(&str) -> Option<String>,
{
    let path = match config::xdg_toml::config_path(CONFIG_APP_NAME) {
        Ok(Some(p)) => p,
        Ok(None) => return Err("config.toml not found".to_string()),
        Err(e) => return Err(format!("config path error: {e}")),
    };

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read config.toml: {e}"))?;

    let new_content = transform(&content)
        .ok_or_else(|| format!("provider '{provider_id}' not found in config.toml"))?;

    let tmp_path = path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, &new_content)
        .map_err(|e| format!("failed to write temp file: {e}"))?;
    std::fs::rename(&tmp_path, &path)
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("failed to rename config.toml: {e}")
        })?;

    Ok(())
}

/// Set a field value for a specific provider in the raw TOML text.
/// Handles `[[providers]]` blocks with `name = "..."` headers.
fn set_provider_field(toml: &str, provider_id: &str, field: &str, value: &str) -> Option<String> {
    let provider_header = format!("name = \"{}\"", provider_id);
    let escaped_value = value.replace('\\', "\\\\").replace('"', "\\\"");

    let mut lines: Vec<String> = toml.lines().map(|l| l.to_string()).collect();
    let mut in_providers_block = false;

    for i in 0..lines.len() {
        let trimmed = lines[i].trim();

        if trimmed == "[[providers]]" {
            in_providers_block = true;
            continue;
        }

        if in_providers_block && trimmed.starts_with("[[") {
            in_providers_block = false;
        }

        if in_providers_block && trimmed.starts_with("name =") && trimmed.contains(&provider_header) {
            for j in (i + 1)..lines.len() {
                let t = lines[j].trim();
                if t.starts_with("[[") || t.starts_with('[') {
                    let field_line = format!("{} = \"{}\"", field, escaped_value);
                    lines.insert(j, field_line);
                    return Some(lines.join("\n") + "\n");
                }
                if t.starts_with(&format!("{} =", field)) {
                    lines[j] = format!("{} = \"{}\"", field, escaped_value);
                    return Some(lines.join("\n") + "\n");
                }
            }
            let field_line = format!("{} = \"{}\"", field, escaped_value);
            lines.push(field_line);
            return Some(lines.join("\n") + "\n");
        }
    }

    None
}

/// Remove a field for a specific provider in the raw TOML text.
fn remove_provider_field(toml: &str, provider_id: &str, field: &str) -> Option<String> {
    let provider_header = format!("name = \"{}\"", provider_id);

    let mut lines: Vec<String> = toml.lines().map(|l| l.to_string()).collect();
    let mut in_providers_block = false;

    for i in 0..lines.len() {
        let trimmed = lines[i].trim();

        if trimmed == "[[providers]]" {
            in_providers_block = true;
            continue;
        }

        if in_providers_block && trimmed.starts_with("[[") {
            in_providers_block = false;
        }

        if in_providers_block && trimmed.starts_with("name =") && trimmed.contains(&provider_header) {
            for j in (i + 1)..lines.len() {
                let t = lines[j].trim();
                if t.starts_with("[[") || (t.starts_with('[') && !t.starts_with("[[")) {
                    break;
                }
                if t.starts_with(&format!("{} =", field)) {
                    lines.remove(j);
                    return Some(lines.join("\n") + "\n");
                }
            }
            return Some(lines.join("\n") + "\n");
        }
    }

    None
}
