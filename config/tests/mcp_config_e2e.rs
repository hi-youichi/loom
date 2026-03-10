//! L1 e2e: discover → read file → parse in real temp dirs and env.
//! No dependency on loom.

use config::{discover_mcp_config_path, load_mcp_config_from_path, McpConfigError};
use std::path::Path;

fn restore_xdg(prev: Option<String>) {
    match prev {
        Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }
}

#[test]
fn e2e_discover_then_load_override() {
    let dir = tempfile::tempdir().unwrap();
    let custom = dir.path().join("custom.json");
    let content = r#"{"mcpServers":{"one":{"command":"cmd","args":["a","b"]}}}"#;
    std::fs::write(&custom, content).unwrap();
    let working = dir.path().join("proj");
    std::fs::create_dir_all(working.join(".loom")).unwrap();
    std::fs::write(working.join(".loom").join("mcp.json"), "{}").unwrap();

    let path = discover_mcp_config_path(Some(&custom), Some(working.as_path())).unwrap();
    assert_eq!(path.as_path(), custom);

    let list = load_mcp_config_from_path(&path).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "one");
    assert_eq!(list[0].command, "cmd");
    assert_eq!(list[0].args, ["a", "b"]);
}

#[test]
fn e2e_discover_then_load_project() {
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().join("proj");
    std::fs::create_dir_all(working.join(".loom")).unwrap();
    let project_mcp = working.join(".loom").join("mcp.json");
    let content = r#"{"mcpServers":{"proj-server":{"command":"node","args":["server.js"]}}}"#;
    std::fs::write(&project_mcp, content).unwrap();

    let xdg = dir.path().join("xdg");
    std::fs::create_dir_all(xdg.join("loom")).unwrap();
    std::fs::write(xdg.join("loom").join("mcp.json"), "{}").unwrap();
    let prev = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("XDG_CONFIG_HOME", xdg);

    let path = discover_mcp_config_path(None::<&Path>, Some(working.as_path())).unwrap();
    restore_xdg(prev);
    assert_eq!(path.as_path(), project_mcp);

    let list = load_mcp_config_from_path(&path).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "proj-server");
    assert_eq!(list[0].command, "node");
    assert_eq!(list[0].args, ["server.js"]);
}

#[test]
fn e2e_discover_then_load_global() {
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().join("empty");
    std::fs::create_dir_all(&working).unwrap();
    let xdg = dir.path().join("xdg");
    std::fs::create_dir_all(xdg.join("loom")).unwrap();
    let global_mcp = xdg.join("loom").join("mcp.json");
    let content = r#"{"mcpServers":{"global":{"command":"npx","args":["-y","mcp-server"]}}}"#;
    std::fs::write(&global_mcp, content).unwrap();

    let prev = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("XDG_CONFIG_HOME", &xdg);

    let path = discover_mcp_config_path(None::<&Path>, Some(working.as_path())).unwrap();
    restore_xdg(prev);
    assert_eq!(path.as_path(), global_mcp);

    let list = load_mcp_config_from_path(&path).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "global");
    assert_eq!(list[0].command, "npx");
    assert_eq!(list[0].args, ["-y", "mcp-server"]);
}

#[test]
fn e2e_discover_order_override_over_project() {
    let dir = tempfile::tempdir().unwrap();
    let override_path = dir.path().join("override.json");
    std::fs::write(&override_path, r#"{"mcpServers":{"ov":{"command":"ov"}}}"#).unwrap();
    let working = dir.path().join("proj");
    std::fs::create_dir_all(working.join(".loom")).unwrap();
    std::fs::write(working.join(".loom").join("mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
    let xdg = dir.path().join("xdg");
    std::fs::create_dir_all(xdg.join("loom")).unwrap();
    std::fs::write(xdg.join("loom").join("mcp.json"), "{}").unwrap();

    let prev = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("XDG_CONFIG_HOME", &xdg);

    let path = discover_mcp_config_path(Some(&override_path), Some(working.as_path())).unwrap();
    restore_xdg(prev);
    assert_eq!(path.as_path(), override_path);
}

#[test]
fn e2e_discover_order_project_over_global() {
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().join("proj");
    std::fs::create_dir_all(working.join(".loom")).unwrap();
    let project_mcp = working.join(".loom").join("mcp.json");
    std::fs::write(&project_mcp, r#"{"mcpServers":{"p":{"command":"p"}}}"#).unwrap();
    let xdg = dir.path().join("xdg");
    std::fs::create_dir_all(xdg.join("loom")).unwrap();
    std::fs::write(xdg.join("loom").join("mcp.json"), r#"{"mcpServers":{"g":{"command":"g"}}}"#).unwrap();

    let prev = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("XDG_CONFIG_HOME", &xdg);

    let path = discover_mcp_config_path(None::<&Path>, Some(working.as_path())).unwrap();
    restore_xdg(prev);
    assert_eq!(path.as_path(), project_mcp);
}

#[test]
fn e2e_load_invalid_json_returns_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, r#"{"mcpServers": {"x": "not an object"}}"#).unwrap();

    let err = load_mcp_config_from_path(&path).unwrap_err();
    assert!(matches!(err, McpConfigError::Parse(_)));
}

#[test]
fn e2e_load_disabled_filtered_out() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    let content = r#"{
        "mcpServers": {
            "enabled": {"command": "c1", "args": []},
            "off": {"command": "c2", "args": [], "disabled": true}
        }
    }"#;
    std::fs::write(&path, content).unwrap();

    let list = load_mcp_config_from_path(&path).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "enabled");
    assert_eq!(list[0].command, "c1");
}

#[test]
fn e2e_discover_none_when_nothing_exists() {
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().join("empty");
    std::fs::create_dir_all(&working).unwrap();

    let prev = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("XDG_CONFIG_HOME", dir.path());

    let path = discover_mcp_config_path(None::<&Path>, Some(working.as_path()));
    restore_xdg(prev);

    assert!(path.is_none());
}
