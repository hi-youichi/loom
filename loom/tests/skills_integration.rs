//! Integration tests for skills discovery and injection in build_helve_config.

use loom::{build_helve_config, RunOptions};

fn opts(working_folder: std::path::PathBuf) -> RunOptions {
    RunOptions {
        message: loom::UserContent::text(String::new()),
        working_folder: Some(working_folder),
        session_id: None,
        thread_id: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
        model: None,
        provider: None,
        base_url: None,
        api_key: None,
        provider_type: None,
        mcp_config_path: None,
        cancellation: None,
        output_timestamp: false,
        dry_run: false,
    }
}

/// Scenario: working_folder has .loom/skills/code-review/SKILL.md with front matter.
/// build_helve_config produces skills_prompt and config.skill_registry with the skill.
#[test]
fn build_helve_config_discovers_skills_and_injects_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join(".loom").join("skills");
    std::fs::create_dir_all(skills_dir.join("code-review")).unwrap();
    let skill_md = r#"---
name: code-review
description: Review code for quality and security. Use when reviewing PRs.
---

# Code Review

## Instructions
Check correctness, security, and style.
"#;
    std::fs::write(skills_dir.join("code-review").join("SKILL.md"), skill_md).unwrap();

    let run_opts = opts(dir.path().to_path_buf());
    let (helve, config, _resolved_agent) = build_helve_config(&run_opts);

    assert!(
        helve
            .skills_prompt
            .as_ref()
            .map_or(false, |s| s.contains("code-review")
                && s.contains("Review code")),
        "expected skills_prompt to contain skill name and description, got: {:?}",
        helve.skills_prompt
    );
    assert!(
        config.skill_registry.is_some(),
        "expected skill_registry to be set"
    );
    let registry = config.skill_registry.as_ref().unwrap();
    let list = registry.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].metadata.name, "code-review");
    let content = registry.load_skill("code-review").unwrap();
    assert!(content.contains("Code Review"));
    assert!(content.contains("Instructions"));
}

/// Scenario: no .loom/skills directory → no skills_prompt, no skill_registry.
#[test]
fn build_helve_config_no_skills_dir_no_prompt() {
    let dir = tempfile::tempdir().unwrap();
    // Isolate from real ~/.loom/skills
    let prev = std::env::var("LOOM_HOME").ok();
    std::env::set_var("LOOM_HOME", dir.path().join("empty_loom_home"));
    let run_opts = opts(dir.path().to_path_buf());
    let (helve, config, _resolved_agent) = build_helve_config(&run_opts);
    match prev {
        Some(v) => std::env::set_var("LOOM_HOME", v),
        None => std::env::remove_var("LOOM_HOME"),
    }

    assert!(helve.skills_prompt.is_none());
    if let Some(ref reg) = config.skill_registry {
        assert!(reg.list().is_empty(), "expected empty skill registry");
    }
}
