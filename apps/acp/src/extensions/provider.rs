//! Provider catalog and API-key management for Loom ACP clients.
//!
//! Provider definitions live in `{loom_home}/config.toml` as `[[providers]]`.
//! Secrets are write-only through this extension: responses never contain the
//! configured API key.

use std::fs;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

fn config_path() -> PathBuf {
    config::home::loom_home().join("config.toml")
}

fn internal(message: impl Into<String>) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}

fn read_document() -> Result<toml::Table, ExtensionError> {
    let path = config_path();
    if !path.exists() {
        return Ok(toml::Table::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|error| internal(format!("failed to read Loom config: {error}")))?;
    content
        .parse::<toml::Table>()
        .map_err(|error| internal(format!("failed to parse Loom config: {error}")))
}

fn write_document(document: &toml::Table) -> Result<(), ExtensionError> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            internal(format!("failed to create Loom config directory: {error}"))
        })?;
    }
    let content = toml::to_string_pretty(document)
        .map_err(|error| internal(format!("failed to serialize Loom config: {error}")))?;
    let temporary = path.with_extension(format!("toml.tmp-{}", std::process::id()));
    fs::write(&temporary, content)
        .map_err(|error| internal(format!("failed to write Loom config: {error}")))?;
    fs::rename(&temporary, &path)
        .map_err(|error| internal(format!("failed to replace Loom config: {error}")))
}

fn provider_id(params: &Value) -> Result<String, ExtensionError> {
    params
        .get("providerID")
        .or_else(|| params.get("providerId"))
        .or_else(|| params.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ExtensionError::invalid_params("providerID is required"))
}

fn provider_name(value: &toml::Value) -> Option<&str> {
    value.get("name").and_then(toml::Value::as_str)
}

fn provider_values(document: &toml::Table) -> Vec<&toml::Value> {
    document
        .get("providers")
        .and_then(toml::Value::as_array)
        .map(|values| values.iter().collect())
        .unwrap_or_default()
}

fn provider_json(value: &toml::Value) -> Option<Value> {
    let table = value.as_table()?;
    let id = table.get("name")?.as_str()?.to_string();
    let mut models = table
        .get("models")
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("id").and_then(toml::Value::as_str))
                .map(|model| json!({ "id": model, "name": model }))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(model) = table.get("model").and_then(toml::Value::as_str) {
        if !models
            .iter()
            .any(|entry| entry.get("id").and_then(Value::as_str) == Some(model))
        {
            models.insert(0, json!({ "id": model, "name": model }));
        }
    }
    let configured = table
        .get("api_key")
        .and_then(toml::Value::as_str)
        .map(|key| !key.is_empty())
        .unwrap_or(false);
    Some(json!({ "id": id, "name": id, "models": models, "configured": configured }))
}

pub struct ProviderHandler;

#[async_trait]
impl ExtensionHandler for ProviderHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        _ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => {
                let document = read_document()?;
                let providers = provider_values(&document)
                    .into_iter()
                    .filter_map(provider_json)
                    .collect::<Vec<_>>();
                Ok(json!({ "providers": providers }))
            }
            "auth_methods" => {
                let document = read_document()?;
                let mut result = serde_json::Map::new();
                for value in provider_values(&document) {
                    if let Some(id) = provider_name(value) {
                        result.insert(
                            id.to_string(),
                            json!([{ "type": "api", "name": "API key", "label": "API key" }]),
                        );
                    }
                }
                Ok(Value::Object(result))
            }
            "auth_set" => {
                let id = provider_id(&params)?;
                let key = params
                    .get("key")
                    .or_else(|| params.pointer("/auth/key"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| ExtensionError::invalid_params("API key is required"))?;
                let mut document = read_document()?;
                let values = document
                    .entry("providers")
                    .or_insert_with(|| toml::Value::Array(Vec::new()))
                    .as_array_mut()
                    .ok_or_else(|| {
                        ExtensionError::invalid_params("Loom config 'providers' must be an array")
                    })?;
                let index = values
                    .iter()
                    .position(|value| provider_name(value) == Some(id.as_str()));
                let table = if let Some(index) = index {
                    values[index].as_table_mut().ok_or_else(|| {
                        ExtensionError::invalid_params("provider entry must be a table")
                    })?
                } else {
                    let mut table = toml::Table::new();
                    table.insert("name".into(), toml::Value::String(id.clone()));
                    values.push(toml::Value::Table(table));
                    values
                        .last_mut()
                        .and_then(toml::Value::as_table_mut)
                        .ok_or_else(|| internal("provider entry vanished after creation"))?
                };
                table.insert("api_key".into(), toml::Value::String(key.to_string()));
                write_document(&document)?;
                super::model::ModelHandler::invalidate_cache().await;
                Ok(json!({ "success": true, "id": id, "configured": true }))
            }
            "auth_delete" => {
                let id = provider_id(&params)?;
                let mut document = read_document()?;
                let removed = document
                    .get_mut("providers")
                    .and_then(toml::Value::as_array_mut)
                    .and_then(|values| {
                        values
                            .iter_mut()
                            .find(|value| provider_name(value) == Some(id.as_str()))
                    })
                    .and_then(toml::Value::as_table_mut)
                    .map(|table| table.remove("api_key").is_some())
                    .unwrap_or(false);
                write_document(&document)?;
                super::model::ModelHandler::invalidate_cache().await;
                Ok(json!({ "success": true, "id": id, "removed": removed }))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({ "list": true, "auth_methods": true, "auth_set": true, "auth_delete": true })
    }
}
