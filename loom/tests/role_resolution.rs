//! Integration tests for role_setting resolution in build_helve_config.
//!
//! Covers: --role file (readable non-empty, readable empty) > SOUL.md in working_folder > DEFAULT_SOUL.

use loom::{build_helve_config, RunOptions};

fn opts(working_folder: Option<std::path::PathBuf>, role_file: Option<std::path::PathBuf>) -> RunOptions {
    RunOptions {
        message: String::new(),
        working_folder,
        thread_id: None,
        role_file,
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
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
    let (helve, _) = build_helve_config(&opts);

    assert_eq!(helve.role_setting.as_deref(), Some(content));
}

/// Scenario 2: role_file set, file readable but empty or whitespace-only → fallback to SOUL.md or DEFAULT_SOUL.
#[test]
fn role_file_empty_fallback_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let role_path = dir.path().join("empty_role.md");
    std::fs::write(&role_path, "   \n\t  ").unwrap();
    // No SOUL.md in working_folder → fallback to DEFAULT_SOUL
    let opts = opts(Some(dir.path().to_path_buf()), Some(role_path));
    let (helve, _) = build_helve_config(&opts);

    assert!(helve.role_setting.is_some());
    let s = helve.role_setting.unwrap();
    assert!(s.contains("capable") || s.contains("assistant"), "expected default SOUL content, got: {:?}", s);
}

/// Scenario 5: role_file None, working_folder contains SOUL.md → role_setting from SOUL.md.
#[test]
fn no_role_file_soul_md_in_working_folder_used() {
    let dir = tempfile::tempdir().unwrap();
    let soul_content = "You are a specialized QA bot.";
    std::fs::write(dir.path().join("SOUL.md"), soul_content).unwrap();

    let opts = opts(Some(dir.path().to_path_buf()), None);
    let (helve, _) = build_helve_config(&opts);

    assert_eq!(helve.role_setting.as_deref(), Some(soul_content));
}

/// Scenario 6: role_file None, no SOUL.md in working_folder → role_setting is DEFAULT_SOUL.
#[test]
fn no_role_file_no_soul_md_uses_default_soul() {
    let dir = tempfile::tempdir().unwrap();
    // No SOUL.md created

    let opts = opts(Some(dir.path().to_path_buf()), None);
    let (helve, _) = build_helve_config(&opts);

    assert!(helve.role_setting.is_some());
    let s = helve.role_setting.unwrap();
    assert!(s.contains("capable") || s.contains("assistant"), "expected default SOUL, got: {:?}", s);
}
