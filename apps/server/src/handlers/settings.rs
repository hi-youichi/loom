//! HTTP handlers for `/config/settings` and `/config/reload`.
//!
//! These routes operate on `~/.loom/config.toml` directly — the on-disk
//! configuration file — rather than the in-memory `AppState::config`.
//!
//! | Endpoint               | Method | Description                          |
//! |------------------------|--------|--------------------------------------|
//! | `/config/settings`     | GET    | Return current settings JSON         |
//! | `/config/settings`     | PUT    | Merge partial settings + persist     |
//! | `/config/reload`       | POST   | Reload config from disk              |

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::fs;

const CONFIG_APP_NAME: &str = "loom";

/// Resolve the path to `~/.loom/config.toml`.
fn config_file_path() -> Result<std::path::PathBuf, (StatusCode, Json<Value>)> {
    let path = config::home::loom_home().join("config.toml");
    Ok(path)
}

/// Read and parse the config TOML file into a JSON `Value`.
fn read_config_json() -> Result<Value, (StatusCode, Json<Value>)> {
    let path = config_file_path()?;
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to read config: {e}")})),
            ));
        }
    };
    match toml::from_str::<toml::Value>(&content) {
        Ok(v) => Ok(toml_to_json(&v)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("failed to parse config TOML: {e}")})),
        )),
    }
}

/// Recursively convert a `toml::Value` to `serde_json::Value`.
fn toml_to_json(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s) => json!(s),
        toml::Value::Integer(i) => json!(i),
        toml::Value::Float(f) => json!(f),
        toml::Value::Boolean(b) => json!(b),
        toml::Value::Array(arr) => json!(arr.iter().map(toml_to_json).collect::<Vec<_>>()),
        toml::Value::Table(tbl) => {
            let mut map = serde_json::Map::new();
            for (k, v) in tbl {
                map.insert(k.clone(), toml_to_json(v));
            }
            Value::Object(map)
        }
        toml::Value::Datetime(dt) => json!(dt.to_string()),
    }
}

/// Strip sensitive fields (api_key, token, secret) from a JSON value recursively.
fn strip_sensitive(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.remove("api_key");
            map.remove("apiKey");
            map.remove("token");
            map.remove("secret");
            for (_, child) in map.iter_mut() {
                strip_sensitive(child);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_sensitive(item);
            }
        }
        _ => {}
    }
}

/// Deep-merge `patch` into `target` (mutates `target`).
/// Arrays are replaced, objects are merged recursively.
fn deep_merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            for (key, patch_val) in patch_map {
                match target_map.get_mut(key) {
                    Some(existing) => deep_merge(existing, patch_val),
                    None => {
                        target_map.insert(key.clone(), patch_val.clone());
                    }
                }
            }
        }
        (target, patch) => {
            *target = patch.clone();
        }
    }
}

/// Convert a `serde_json::Value` back to `toml::Value` for serialization.
fn json_to_toml(v: &Value) -> Result<toml::Value, String> {
    match v {
        Value::String(s) => Ok(toml::Value::String(s.clone())),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Ok(toml::Value::String(n.to_string()))
            }
        }
        Value::Bool(b) => Ok(toml::Value::Boolean(*b)),
        Value::Array(arr) => {
            let mut out = Vec::new();
            for item in arr {
                out.push(json_to_toml(item)?);
            }
            Ok(toml::Value::Array(out))
        }
        Value::Object(map) => {
            let mut tbl = toml::value::Table::new();
            for (k, v) in map {
                tbl.insert(k.clone(), json_to_toml(v)?);
            }
            Ok(toml::Value::Table(tbl))
        }
        Value::Null => Ok(toml::Value::String("".to_string())),
    }
}

/// `GET /config/settings` — return current config.toml as JSON with
/// sensitive fields stripped.
pub async fn get_settings() -> Response {
    match read_config_json() {
        Ok(mut settings) => {
            strip_sensitive(&mut settings);
            Json(settings).into_response()
        }
        Err(resp) => resp.into_response(),
    }
}

/// `PUT /config/settings` — merge partial JSON into config.toml and persist.
pub async fn put_settings(Json(body): Json<Value>) -> Response {
    let path = match config_file_path() {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };

    let mut current = match read_config_json() {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };

    deep_merge(&mut current, &body);

    let toml_val = match json_to_toml(&current) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to convert to TOML: {e}")})),
            )
                .into_response();
        }
    };

    let toml_str = match toml::to_string_pretty(&toml_val) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to serialize TOML: {e}")})),
            )
                .into_response();
        }
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to create config dir: {e}")})),
            )
                .into_response();
        }
    }

    if let Err(e) = fs::write(&path, &toml_str) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("failed to write config: {e}")})),
        )
            .into_response();
    }

    let mut result = current.clone();
    strip_sensitive(&mut result);
    Json(json!({"status": "ok", "settings": result})).into_response()
}

/// `POST /config/reload` — reload config from disk. Triggers
/// `config::load_full_config` to verify the config parses cleanly.
pub async fn reload_config() -> Response {
    match config::load_full_config(CONFIG_APP_NAME) {
        Ok(cfg) => {
            let provider_count = cfg.providers.len();
            let default = cfg.default_provider.unwrap_or_else(|| "(none)".to_string());
            Json(json!({
                "status": "ok",
                "message": "config reloaded",
                "providers": provider_count,
                "default_provider": default,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "error": format!("config reload failed: {e}"),
            })),
        )
            .into_response(),
    }
}
