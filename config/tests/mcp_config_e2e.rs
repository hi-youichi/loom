//! L1 e2e: discover → read file → parse in real temp dirs and env.
//! No dependency on loom.

use config::{discover_mcp_config_path, load_mcp_config_from_path, McpConfigError, McpServerDef};
use std::path::Path;
use std::sync::Mutex;

static LOOM_HOME_LOCK: Mutex<()> = Mutex::new(());

fn restore_loom_home(prev: Option<String>) {
    match prev {
        Some(v) => std::env::set_var("LOOM_HOME", v),
        None => std::env::remove_var("LOOM_HOME"),
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
    match &list[0] {
        McpServerDef::Stdio { name, command, args, .. } => {
            assert_eq!(name, "one");
            assert_eq!(command, "cmd");
            assert_eq!(args.as_slice(), ["a", "b"]);
        }
        McpServerDef::Http { .. } => panic!("expected Stdio"),
    }
}

#[test]
fn e2e_discover_then_load_project() {
    let _lock = LOOM_HOME_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().join("proj");
    std::fs::create_dir_all(working.join(".loom")).unwrap();
    let project_mcp = working.join(".loom").join("mcp.json");
    let content = r#"{"mcpServers":{"proj-server":{"command":"node","args":["server.js"]}}}"#;
    std::fs::write(&project_mcp, content).unwrap();

    let loom_home = dir.path().join("loom_home");
    std::fs::create_dir_all(&loom_home).unwrap();
    std::fs::write(loom_home.join("mcp.json"), "{}").unwrap();
    let prev = std::env::var("LOOM_HOME").ok();
    std::env::set_var("LOOM_HOME", &loom_home);

    let path = discover_mcp_config_path(None::<&Path>, Some(working.as_path())).unwrap();
    restore_loom_home(prev);
    assert_eq!(path.as_path(), project_mcp);

    let list = load_mcp_config_from_path(&path).unwrap();
    assert_eq!(list.len(), 1);
    match &list[0] {
        McpServerDef::Stdio { name, command, args, .. } => {
            assert_eq!(name, "proj-server");
            assert_eq!(command, "node");
            assert_eq!(args.as_slice(), ["server.js"]);
        }
        McpServerDef::Http { .. } => panic!("expected Stdio"),
    }
}

#[test]
fn e2e_discover_then_load_global() {
    let _lock = LOOM_HOME_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().join("empty");
    std::fs::create_dir_all(&working).unwrap();
    let loom_home = dir.path().join("loom_home");
    std::fs::create_dir_all(&loom_home).unwrap();
    let global_mcp = loom_home.join("mcp.json");
    let content = r#"{"mcpServers":{"global":{"command":"npx","args":["-y","mcp-server"]}}}"#;
    std::fs::write(&global_mcp, content).unwrap();

    let prev = std::env::var("LOOM_HOME").ok();
    std::env::set_var("LOOM_HOME", &loom_home);

    let path = discover_mcp_config_path(None::<&Path>, Some(working.as_path())).unwrap();
    restore_loom_home(prev);
    assert_eq!(path.as_path(), global_mcp);

    let list = load_mcp_config_from_path(&path).unwrap();
    assert_eq!(list.len(), 1);
    match &list[0] {
        McpServerDef::Stdio { name, command, args, .. } => {
            assert_eq!(name, "global");
            assert_eq!(command, "npx");
            assert_eq!(args.as_slice(), ["-y", "mcp-server"]);
        }
        McpServerDef::Http { .. } => panic!("expected Stdio"),
    }
}

#[test]
fn e2e_discover_order_override_over_project() {
    let _lock = LOOM_HOME_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let override_path = dir.path().join("override.json");
    std::fs::write(&override_path, r#"{"mcpServers":{"ov":{"command":"ov"}}}"#).unwrap();
    let working = dir.path().join("proj");
    std::fs::create_dir_all(working.join(".loom")).unwrap();
    std::fs::write(working.join(".loom").join("mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
    let loom_home = dir.path().join("loom_home");
    std::fs::create_dir_all(&loom_home).unwrap();
    std::fs::write(loom_home.join("mcp.json"), "{}").unwrap();

    let prev = std::env::var("LOOM_HOME").ok();
    std::env::set_var("LOOM_HOME", &loom_home);

    let path = discover_mcp_config_path(Some(&override_path), Some(working.as_path())).unwrap();
    restore_loom_home(prev);
    assert_eq!(path.as_path(), override_path);
}

#[test]
fn e2e_discover_order_project_over_global() {
    let _lock = LOOM_HOME_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().join("proj");
    std::fs::create_dir_all(working.join(".loom")).unwrap();
    let project_mcp = working.join(".loom").join("mcp.json");
    std::fs::write(&project_mcp, r#"{"mcpServers":{"p":{"command":"p"}}}"#).unwrap();
    let loom_home = dir.path().join("loom_home");
    std::fs::create_dir_all(&loom_home).unwrap();
    std::fs::write(loom_home.join("mcp.json"), r#"{"mcpServers":{"g":{"command":"g"}}}"#).unwrap();

    let prev = std::env::var("LOOM_HOME").ok();
    std::env::set_var("LOOM_HOME", &loom_home);

    let path = discover_mcp_config_path(None::<&Path>, Some(working.as_path())).unwrap();
    restore_loom_home(prev);
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
    match &list[0] {
        McpServerDef::Stdio { name, command, .. } => {
            assert_eq!(name, "enabled");
            assert_eq!(command, "c1");
        }
        McpServerDef::Http { .. } => panic!("expected Stdio"),
    }
}

#[test]
fn e2e_discover_none_when_nothing_exists() {
    let _lock = LOOM_HOME_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let working = dir.path().join("empty");
    std::fs::create_dir_all(&working).unwrap();

    let loom_home = dir.path().join("loom_home");
    std::fs::create_dir_all(&loom_home).unwrap();
    let prev = std::env::var("LOOM_HOME").ok();
    std::env::set_var("LOOM_HOME", &loom_home);

    let path = discover_mcp_config_path(None::<&Path>, Some(working.as_path()));
    restore_loom_home(prev);

    assert!(path.is_none());
}
