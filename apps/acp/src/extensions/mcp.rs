use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::auth;
use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

use config::{
    load_mcp_config_file, upsert_mcp_server, McpConfigError, McpConfigFile, McpServerEntry,
};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

pub struct McpHandler;

impl McpHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for McpHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for McpHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => handle_list(params, ctx).await,
            "get" => handle_get(params, ctx).await,
            "configure" => handle_configure(params, ctx).await,
            "enable" => handle_enable(params, ctx).await,
            "disable" => handle_disable(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({
            "list": true,
            "get": true,
            "configure": true,
            "enable": true,
            "disable": true
        })
    }
}

// ── Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInfo {
    pub id: String,
    pub name: String,
    pub transport: McpTransportInfo,
    pub enabled: bool,
    pub status: McpServerStatus,
    pub tool_count: u32,
    pub last_connected: Option<String>,
    pub scope: McpScope,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransportInfo {
    Stdio {
        command: String,
        args: Vec<String>,
    },
    Sse {
        url: String,
    },
    #[serde(rename = "websocket")]
    WebSocket {
        url: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerStatus {
    Connected,
    Disconnected,
    Error,
    Starting,
    Disabled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransportInput {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Sse {
        url: String,
    },
    WebSocket {
        url: String,
    },
}

struct McpConfigSnapshot {
    entries: Vec<(String, McpServerEntry, McpScope)>,
}

// ── Config path helpers ────────────────────────────────────────────────

fn global_config_path() -> PathBuf {
    config::home::anureo_home().join("mcp.json")
}

fn project_config_path(ctx: &ExtensionContext) -> Option<PathBuf> {
    ctx.working_directory
        .as_ref()
        .map(|wd| wd.join(".anureo").join("mcp.json"))
}

fn load_config_file(path: &Path) -> Result<McpConfigFile, ExtensionError> {
    if !path.exists() {
        return Ok(McpConfigFile {
            mcp_servers: HashMap::new(),
        });
    }
    load_mcp_config_file(path).map_err(|e| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "failed to read mcp config at {}: {e}",
            path.display()
        ))),
    })
}

fn load_snapshot(ctx: &ExtensionContext) -> Result<McpConfigSnapshot, ExtensionError> {
    let global_path = global_config_path();
    let global_config = load_config_file(&global_path)?;

    let project_config = if let Some(proj_path) = project_config_path(ctx) {
        load_config_file(&proj_path)?
    } else {
        McpConfigFile {
            mcp_servers: HashMap::new(),
        }
    };

    let mut seen = std::collections::HashSet::new();
    let mut entries: Vec<(String, McpServerEntry, McpScope)> = Vec::new();

    for (id, entry) in &project_config.mcp_servers {
        seen.insert(id.clone());
        entries.push((id.clone(), entry.clone(), McpScope::Project));
    }

    for (id, entry) in &global_config.mcp_servers {
        if !seen.contains(id) {
            entries.push((id.clone(), entry.clone(), McpScope::Global));
        }
    }

    Ok(McpConfigSnapshot { entries })
}

fn entry_to_info(id: &str, entry: &McpServerEntry, scope: McpScope) -> McpServerInfo {
    let transport = if let Some(ref url) = entry.url {
        McpTransportInfo::Sse { url: url.clone() }
    } else if let Some(ref command) = entry.command {
        McpTransportInfo::Stdio {
            command: command.clone(),
            args: entry.args.clone(),
        }
    } else {
        McpTransportInfo::Stdio {
            command: String::new(),
            args: vec![],
        }
    };

    let enabled = !entry.disabled;
    let status = if entry.disabled {
        McpServerStatus::Disabled
    } else {
        McpServerStatus::Disconnected
    };

    McpServerInfo {
        id: id.to_string(),
        name: id.to_string(),
        transport,
        enabled,
        status,
        tool_count: 0,
        last_connected: None,
        scope,
    }
}

fn find_entry(
    ctx: &ExtensionContext,
    id: &str,
) -> Result<(McpServerEntry, McpScope, PathBuf), ExtensionError> {
    let proj_path = project_config_path(ctx);

    if let Some(ref proj_path) = proj_path {
        if proj_path.exists() {
            let config = load_config_file(proj_path)?;
            if let Some(entry) = config.mcp_servers.get(id) {
                return Ok((entry.clone(), McpScope::Project, proj_path.clone()));
            }
        }
    }

    let global_path = global_config_path();
    if global_path.exists() {
        let config = load_config_file(&global_path)?;
        if let Some(entry) = config.mcp_servers.get(id) {
            return Ok((entry.clone(), McpScope::Global, global_path));
        }
    }

    Err(ExtensionError::not_found(format!(
        "mcp server '{id}' not found"
    )))
}

fn config_err_to_ext(e: McpConfigError, path: &std::path::Path) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!(
            "config operation failed at {}: {e}",
            path.display()
        ))),
    }
}

// ── Method handlers ────────────────────────────────────────────────────

async fn handle_list(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let scope_filter: Option<String> = params
        .get("scope")
        .filter(|v| !v.is_null())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let pagination: PaginationParams = serde_json::from_value(params.clone())
        .map_err(|e| ExtensionError::invalid_params(format!("invalid pagination params: {e}")))?;

    let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
    let cursor_data: Option<serde_json::Map<String, Value>> = pagination
        .decode_cursor()
        .map_err(|_| ExtensionError::invalid_params("invalid cursor"))?;
    let offset = cursor_data
        .and_then(|m| m.get("offset").and_then(|v| v.as_u64()))
        .map(|v| v as usize)
        .unwrap_or(0);

    let snapshot = load_snapshot(ctx)?;

    let items: Vec<McpServerInfo> = snapshot
        .entries
        .iter()
        .filter(|(_, _, scope)| match &scope_filter {
            Some(f) if f == "global" => matches!(scope, McpScope::Global),
            Some(f) if f == "project" => matches!(scope, McpScope::Project),
            _ => true,
        })
        .map(|(id, entry, scope)| entry_to_info(id, entry, scope.clone()))
        .collect();

    let result = PaginatedResult::from_slice(items, offset, limit);
    Ok(result.to_json())
}

async fn handle_get(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    let id: String = params
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ExtensionError::invalid_params("missing required parameter: id"))?;

    if id.is_empty() {
        return Err(ExtensionError::invalid_params("id must not be empty"));
    }

    let (entry, scope, _) = find_entry(ctx, &id)?;

    let info = entry_to_info(&id, &entry, scope);

    let mut result = serde_json::to_value(&info).unwrap_or(Value::Null);
    result["lastError"] = Value::Null;
    result["tools"] = serde_json::json!([]);
    Ok(result)
}

async fn handle_configure(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "mcp", "configure")?;

    let id: String = params
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ExtensionError::invalid_params("missing required parameter: id"))?;

    if id.is_empty() {
        return Err(ExtensionError::invalid_params("id must not be empty"));
    }

    let transport: Option<McpTransportInput> = params
        .get("transport")
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .map(|t| match t {
            McpTransportInput::WebSocket { .. } => Err(ExtensionError::invalid_params(
                "WebSocket transport is not supported in v1",
            )),
            other => Ok(other),
        })
        .transpose()?;

    if let Some(McpTransportInput::Stdio { ref command, .. }) = transport {
        if command.is_empty() {
            return Err(ExtensionError::invalid_params(
                "stdio transport requires non-empty 'command'",
            ));
        }
    }
    if let Some(McpTransportInput::Sse { ref url }) = transport {
        if url.is_empty() {
            return Err(ExtensionError::invalid_params(
                "sse transport requires non-empty 'url'",
            ));
        }
    }

    let enabled: Option<bool> = params
        .get("enabled")
        .filter(|v| !v.is_null())
        .and_then(|v| v.as_bool());

    let overwrite: bool = params
        .get("overwrite")
        .filter(|v| !v.is_null())
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let target_path = project_config_path(ctx).ok_or_else(|| ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(
            "no working directory available for project config".into(),
        )),
    })?;

    let existing_config = if target_path.exists() {
        load_config_file(&target_path)?
    } else {
        McpConfigFile {
            mcp_servers: HashMap::new(),
        }
    };

    let id_exists = existing_config.mcp_servers.contains_key(&id);
    if id_exists && !overwrite {
        return Err(ExtensionError::conflict(format!(
            "already_exists: mcp server '{id}' already exists; use overwrite=true to update"
        )));
    }

    let mut new_entry = if id_exists {
        existing_config.mcp_servers.get(&id).cloned().unwrap()
    } else {
        McpServerEntry {
            command: None,
            args: vec![],
            env: HashMap::new(),
            disabled: false,
            url: None,
            headers: HashMap::new(),
            oauth: None,
            required: false,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
        }
    };

    if let Some(transport) = transport {
        match transport {
            McpTransportInput::Stdio { command, args, env } => {
                new_entry.command = Some(command);
                new_entry.args = args;
                new_entry.env = env;
                new_entry.url = None;
            }
            McpTransportInput::Sse { url } => {
                new_entry.url = Some(url);
                new_entry.command = None;
                new_entry.args = vec![];
            }
            McpTransportInput::WebSocket { .. } => {
                return Err(ExtensionError::invalid_params(
                    "WebSocket transport is not supported in v1",
                ));
            }
        }
    }

    if let Some(enabled) = enabled {
        new_entry.disabled = !enabled;
    }

    upsert_mcp_server(&target_path, &id, new_entry.clone())
        .map_err(|e| config_err_to_ext(e, &target_path))?;

    let status = if new_entry.disabled {
        McpServerStatus::Disabled
    } else {
        McpServerStatus::Starting
    };

    Ok(serde_json::json!({
        "id": id,
        "configured": true,
        "status": status,
    }))
}

async fn handle_enable(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "mcp", "enable")?;

    let id: String = params
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ExtensionError::invalid_params("missing required parameter: id"))?;

    if id.is_empty() {
        return Err(ExtensionError::invalid_params("id must not be empty"));
    }

    let (mut entry, _scope, config_path) = find_entry(ctx, &id)?;

    entry.disabled = false;

    upsert_mcp_server(&config_path, &id, entry).map_err(|e| config_err_to_ext(e, &config_path))?;

    Ok(serde_json::json!({
        "id": id,
        "enabled": true,
        "status": McpServerStatus::Starting,
    }))
}

async fn handle_disable(params: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
    auth::check_server_policy(ctx, "mcp", "disable")?;

    let id: String = params
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ExtensionError::invalid_params("missing required parameter: id"))?;

    if id.is_empty() {
        return Err(ExtensionError::invalid_params("id must not be empty"));
    }

    let (mut entry, _scope, config_path) = find_entry(ctx, &id)?;

    entry.disabled = true;

    upsert_mcp_server(&config_path, &id, entry).map_err(|e| config_err_to_ext(e, &config_path))?;

    Ok(serde_json::json!({
        "id": id,
        "enabled": false,
        "status": McpServerStatus::Disabled,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    fn make_ctx(dir: &std::path::Path, principal: &str) -> ExtensionContext {
        ExtensionContext {
            session_id: Some("test-session".into()),
            principal: principal.into(),
            connection_id: "test-conn".into(),
            working_directory: Some(dir.to_path_buf()),
            client_capabilities: ClientCapabilitiesInfo::default(),
        }
    }

    fn write_project_config(dir: &std::path::Path, json: &str) {
        let anureo_dir = dir.join(".anureo");
        fs::create_dir_all(&anureo_dir).unwrap();
        fs::write(anureo_dir.join("mcp.json"), json).unwrap();
    }

    fn write_global_config(anureo_home: &std::path::Path, json: &str) {
        fs::write(anureo_home.join("mcp.json"), json).unwrap();
    }

    fn setup_env() -> (TempDir, TempDir) {
        let project_dir = TempDir::new().unwrap();
        let anureo_home = TempDir::new().unwrap();
        config::home::set_override(Some(anureo_home.path().to_path_buf()));
        (project_dir, anureo_home)
    }

    fn restore_env() {
        config::home::set_override(None);
    }

    #[tokio::test]
    #[serial]
    async fn test_list_merged_global_and_project() {
        let (project_dir, anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"fs":{"command":"npx","args":["-y","fs-server"]}}}"#,
        );
        write_global_config(
            anureo_home.path(),
            r#"{"mcpServers":{"github":{"url":"https://api.github.com/sse"}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);

        let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"fs"));
        assert!(ids.contains(&"github"));

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_list_scope_filter_global() {
        let (project_dir, anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"fs":{"command":"npx","args":[]}}}"#,
        );
        write_global_config(
            anureo_home.path(),
            r#"{"mcpServers":{"github":{"url":"https://api.github.com/sse"}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"scope": "global"}), &ctx)
            .await
            .unwrap();

        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "github");
        assert_eq!(items[0]["scope"], "global");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_list_scope_filter_project() {
        let (project_dir, anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"fs":{"command":"npx","args":[]}}}"#,
        );
        write_global_config(
            anureo_home.path(),
            r#"{"mcpServers":{"github":{"url":"https://api.github.com/sse"}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"scope": "project"}), &ctx)
            .await
            .unwrap();

        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "fs");
        assert_eq!(items[0]["scope"], "project");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_list_pagination() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let config = r#"{"mcpServers":{
            "a":{"command":"cmd-a","args":[]},
            "b":{"command":"cmd-b","args":[]},
            "c":{"command":"cmd-c","args":[]}
        }}"#;
        write_project_config(project_dir.path(), config);

        let handler = McpHandler::new();
        let result = handler
            .handle("list", serde_json::json!({"limit": 1}), &ctx)
            .await
            .unwrap();

        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert!(result["hasMore"].as_bool().unwrap());
        assert!(result["nextCursor"].as_str().is_some());

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_list_empty() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 0);
        assert!(!result["hasMore"].as_bool().unwrap());
        assert!(result["nextCursor"].is_null());

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_list_never_includes_env() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"fs":{"command":"npx","args":[],"env":{"API_KEY":"secret123"}}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        let transport = &items[0]["transport"];
        assert_eq!(transport["type"], "stdio");
        assert!(transport.get("env").is_none());
        let transport_str = transport.to_string();
        assert!(!transport_str.contains("secret123"));

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_list_transport_has_type_tag() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"std":{"command":"npx","args":[]},"remote":{"url":"https://example.com/sse"}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let items = result["items"].as_array().unwrap();
        for item in items {
            let transport = &item["transport"];
            assert!(
                transport.get("type").is_some(),
                "transport must have 'type' tag"
            );
        }

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_get_existing_server() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"fs":{"command":"npx","args":["-y","fs-server"]}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("get", serde_json::json!({"id": "fs"}), &ctx)
            .await
            .unwrap();

        assert_eq!(result["id"], "fs");
        assert_eq!(result["transport"]["type"], "stdio");
        assert_eq!(result["transport"]["command"], "npx");
        assert!(result.get("lastError").is_some());
        assert_eq!(result["lastError"], serde_json::Value::Null);
        assert!(result.get("tools").is_some());
        assert!(result["tools"].is_array());

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_get_nonexistent_returns_not_found() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle("get", serde_json::json!({"id": "nope"}), &ctx)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_get_empty_id_returns_invalid_params() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle("get", serde_json::json!({"id": ""}), &ctx)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_get_strips_env() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"fs":{"command":"npx","args":[],"env":{"SECRET":"topsecret"}}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("get", serde_json::json!({"id": "fs"}), &ctx)
            .await
            .unwrap();

        let transport = &result["transport"];
        assert!(transport.get("env").is_none());
        let json_str = result.to_string();
        assert!(!json_str.contains("topsecret"));

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_creates_new_server() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "new-server",
                    "transport": {
                        "type": "stdio",
                        "command": "node",
                        "args": ["server.js"],
                        "env": {"API_KEY": "secret"}
                    },
                    "enabled": true
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result["id"], "new-server");
        assert_eq!(result["configured"], true);
        assert_eq!(result["status"], "starting");

        let config_path = project_dir.path().join(".anureo").join("mcp.json");
        let config_content = fs::read_to_string(&config_path).unwrap();
        assert!(config_content.contains("new-server"));
        assert!(config_content.contains("node"));

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_already_exists_conflict() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"existing":{"command":"node","args":[]}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "existing",
                    "transport": {"type": "stdio", "command": "newcmd", "args": []}
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32005);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_overwrite_updates() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"srv":{"command":"oldcmd","args":[]}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "srv",
                    "transport": {"type": "stdio", "command": "newcmd", "args": ["--port", "8080"]},
                    "overwrite": true
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result["id"], "srv");
        assert_eq!(result["configured"], true);

        let config_content =
            fs::read_to_string(project_dir.path().join(".anureo").join("mcp.json")).unwrap();
        assert!(config_content.contains("newcmd"));

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_invalid_transport_empty_command() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "bad",
                    "transport": {"type": "stdio", "command": "", "args": []}
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_websocket_rejected() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "ws-server",
                    "transport": {"type": "web_socket", "url": "ws://localhost:9000"}
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32602);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_enabled_false_returns_disabled() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "srv",
                    "transport": {"type": "stdio", "command": "node", "args": []},
                    "enabled": false
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result["status"], "disabled");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_enabled_none_preserves_disabled_status() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"srv":{"command":"node","args":[],"disabled":true}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "srv",
                    "transport": {"type": "stdio", "command": "node", "args": ["--updated"]},
                    "overwrite": true
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result["status"], "disabled");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_no_principal_forbidden() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "");

        let handler = McpHandler::new();
        let result = handler
            .handle("configure", serde_json::json!({"id": "srv"}), &ctx)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32002);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_sse_transport() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "remote",
                    "transport": {"type": "sse", "url": "https://example.com/sse"}
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result["id"], "remote");
        assert_eq!(result["configured"], true);
        assert_eq!(result["status"], "starting");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_enable_then_get() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"srv":{"command":"node","args":[],"disabled":true}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("enable", serde_json::json!({"id": "srv"}), &ctx)
            .await
            .unwrap();

        assert_eq!(result["id"], "srv");
        assert_eq!(result["enabled"], true);
        assert_eq!(result["status"], "starting");

        let get_result = handler
            .handle("get", serde_json::json!({"id": "srv"}), &ctx)
            .await
            .unwrap();

        assert_eq!(get_result["enabled"], true);
        assert_eq!(get_result["status"], "disconnected");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_disable_then_get() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"srv":{"command":"node","args":[],"disabled":false}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("disable", serde_json::json!({"id": "srv"}), &ctx)
            .await
            .unwrap();

        assert_eq!(result["id"], "srv");
        assert_eq!(result["enabled"], false);
        assert_eq!(result["status"], "disabled");

        let get_result = handler
            .handle("get", serde_json::json!({"id": "srv"}), &ctx)
            .await
            .unwrap();

        assert_eq!(get_result["enabled"], false);
        assert_eq!(get_result["status"], "disabled");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_enable_nonexistent_returns_not_found() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle("enable", serde_json::json!({"id": "ghost"}), &ctx)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_disable_nonexistent_returns_not_found() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        let result = handler
            .handle("disable", serde_json::json!({"id": "ghost"}), &ctx)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32003);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_enable_no_principal_forbidden() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "");

        let handler = McpHandler::new();
        let result = handler
            .handle("enable", serde_json::json!({"id": "srv"}), &ctx)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32002);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_disable_no_principal_forbidden() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "");

        let handler = McpHandler::new();
        let result = handler
            .handle("disable", serde_json::json!({"id": "srv"}), &ctx)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32002);

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_enable_already_enabled_is_idempotent() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"srv":{"command":"node","args":[],"disabled":false}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("enable", serde_json::json!({"id": "srv"}), &ctx)
            .await
            .unwrap();

        assert_eq!(result["enabled"], true);
        assert_eq!(result["status"], "starting");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_disable_already_disabled_is_idempotent() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"srv":{"command":"node","args":[],"disabled":true}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("disable", serde_json::json!({"id": "srv"}), &ctx)
            .await
            .unwrap();

        assert_eq!(result["enabled"], false);
        assert_eq!(result["status"], "disabled");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_configure_then_list_shows_new_server() {
        let (project_dir, _anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        let handler = McpHandler::new();
        handler
            .handle(
                "configure",
                serde_json::json!({
                    "id": "new-srv",
                    "transport": {"type": "stdio", "command": "node", "args": ["srv.js"]}
                }),
                &ctx,
            )
            .await
            .unwrap();

        let list_result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let items = list_result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "new-srv");
        assert_eq!(items[0]["transport"]["type"], "stdio");
        assert_eq!(items[0]["transport"]["command"], "node");

        restore_env();
    }

    #[tokio::test]
    #[serial]
    async fn test_project_overrides_global_same_id() {
        let (project_dir, anureo_home) = setup_env();
        let ctx = make_ctx(project_dir.path(), "test-user");

        write_global_config(
            anureo_home.path(),
            r#"{"mcpServers":{"srv":{"url":"https://global.example.com/sse"}}}"#,
        );
        write_project_config(
            project_dir.path(),
            r#"{"mcpServers":{"srv":{"command":"local","args":[]}}}"#,
        );

        let handler = McpHandler::new();
        let result = handler
            .handle("list", serde_json::json!({}), &ctx)
            .await
            .unwrap();

        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "srv");
        assert_eq!(items[0]["scope"], "project");
        assert_eq!(items[0]["transport"]["type"], "stdio");
        assert_eq!(items[0]["transport"]["command"], "local");

        restore_env();
    }

    #[tokio::test]
    async fn test_method_not_found() {
        let ctx = ExtensionContext {
            session_id: Some("test".into()),
            principal: "user".into(),
            connection_id: "conn".into(),
            working_directory: None,
            client_capabilities: ClientCapabilitiesInfo::default(),
        };

        let handler = McpHandler::new();
        let result = handler
            .handle("unknown_method", serde_json::json!({}), &ctx)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, -32601);
    }
}
