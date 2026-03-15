//! Integration tests for role_setting resolution in build_helve_config.
//!
//! Covers: --role file (readable non-empty, readable empty) > instructions.md (or SOUL.md) in working_folder > built-in default.

use loom::{build_helve_config, RunOptions};

fn opts(working_folder: Option<std::path::PathBuf>, role_file: Option<std::path::PathBuf>) -> RunOptions {
    RunOptions {
        message: String::new(),
        working_folder,
        session_id: None,
        thread_id: None,
        role_file,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
        model: None,
        mcp_config_path: None,
    }
}

/// Scenario 1: role_file set, file readable and non-empty → role_setting is that file's content (trimmed).
#[test]
fn role_file_readable_non_empty_used() {
    let dir = tempfile::tempdir().unwrap();
    let role_path = dir.path().join("my_role.md");
    let content = "You are a code review expert.";
    std::fs::write(&role_path, content).unwrap();

    let opts = opts(Some(dir.path().to_path_buf()), Some(role_path));
    let (helve, _, _) = build_helve_config(&opts);

    assert_eq!(helve.role_setting.as_deref(), Some(content));
}

/// Scenario 2: role_file set, file readable but empty or whitespace-only → fallback to instructions.md or built-in default.
#[test]
fn role_file_empty_fallback_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let role_path = dir.path().join("empty_role.md");
    std::fs::write(&role_path, "   \n\t  ").unwrap();
    // No instructions.md in working_folder → fallback to built-in default
    let opts = opts(Some(dir.path().to_path_buf()), Some(role_path));
    let (helve, _, _) = build_helve_config(&opts);

    assert!(helve.role_setting.is_some());
    let s = helve.role_setting.unwrap();
    assert!(s.contains("agent") || s.contains("CLI"), "expected default instructions content, got: {:?}", s);
}

/// Scenario 5: role_file None, working_folder contains instructions.md (or SOUL.md) → role_setting from that file.
#[test]
fn no_role_file_instructions_in_working_folder_used() {
    let dir = tempfile::tempdir().unwrap();
    let role_content = "You are a specialized QA bot.";
    std::fs::write(dir.path().join("instructions.md"), role_content).unwrap();

    let opts = opts(Some(dir.path().to_path_buf()), None);
    let (helve, _, _) = build_helve_config(&opts);

    assert_eq!(helve.role_setting.as_deref(), Some(role_content));
}

/// Scenario 5b: SOUL.md in working_folder still used when instructions.md absent (legacy).
#[test]
fn no_role_file_soul_md_legacy_used() {
    let dir = tempfile::tempdir().unwrap();
    let role_content = "You are a legacy SOUL bot.";
    std::fs::write(dir.path().join("SOUL.md"), role_content).unwrap();

    let opts = opts(Some(dir.path().to_path_buf()), None);
    let (helve, _, _) = build_helve_config(&opts);

    assert_eq!(helve.role_setting.as_deref(), Some(role_content));
}

/// Scenario 6: role_file None, no instructions.md in working_folder → role_setting is built-in default.
#[test]
fn no_role_file_no_instructions_uses_default() {
    let dir = tempfile::tempdir().unwrap();

    let opts = opts(Some(dir.path().to_path_buf()), None);
    let (helve, _, _) = build_helve_config(&opts);

    assert!(helve.role_setting.is_some());
    let s = helve.role_setting.unwrap();
    assert!(s.contains("agent") || s.contains("CLI"), "expected default instructions, got: {:?}", s);
}
