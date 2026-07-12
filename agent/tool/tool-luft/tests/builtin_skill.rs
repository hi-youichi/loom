//! Integration test: prove the full wiring path works:
//! `LuftTool::builtin_skill() -> SkillRegistry::add_builtin() -> load_skill_with_dir()`

use skill::discovery::{SkillRegistry, SkillSource};
use tool_core::Tool;
use tool_luft::LuftTool;

fn make_tool() -> LuftTool {
    use agent::agent::AgentConfig;
    LuftTool::new(AgentConfig::default())
}

#[test]
fn luft_tool_exposes_builtin_skill() {
    let tool = make_tool();
    let skill = tool
        .builtin_skill()
        .expect("LuftTool should expose a builtin skill");
    assert_eq!(skill.name, "luft-workflow-dsl");
    assert!(skill.triggers.contains(&"luft".to_string()));
    assert!(skill.requires_tools.contains(&"luft".to_string()));
    assert!(skill.content.contains("# Luft Workflow DSL Reference"));
}

#[test]
fn builtin_skill_injects_into_registry() {
    let mut registry = SkillRegistry::empty();
    let tool = make_tool();
    let skill = tool.builtin_skill().unwrap();

    registry.add_builtin(
        &skill.name,
        &skill.description,
        &skill.content,
        skill.triggers.clone(),
        skill.requires_tools.clone(),
    );

    let entries = registry.list();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.source, SkillSource::Builtin);
    assert_eq!(entry.metadata.name, "luft-workflow-dsl");
    assert!(entry.embedded_content.is_some());

    // Triggers preserved
    assert!(entry.metadata.triggers.contains(&"luft".to_string()));
    assert!(entry.metadata.triggers.contains(&"workflow".to_string()));
}

#[test]
fn builtin_skill_loadable_via_registry() {
    let mut registry = SkillRegistry::empty();
    let tool = make_tool();
    let skill = tool.builtin_skill().unwrap();

    registry.add_builtin(
        &skill.name,
        &skill.description,
        &skill.content,
        skill.triggers,
        skill.requires_tools,
    );

    let (content, _) = registry
        .load_skill_with_dir("luft-workflow-dsl")
        .expect("builtin skill should load");

    // Frontmatter stripped, body present
    assert!(content.contains("# Luft Workflow DSL Reference"));
    assert!(content.contains("## Required Structure"));
    assert!(content.contains("agent(opts)"));
    // YAML frontmatter (with name: luft-workflow-dsl) is removed.
    // Note: markdown horizontal rules `---` in the body are kept.
    assert!(!content.contains("name: luft-workflow-dsl"));
}

#[test]
fn available_skills_prompt_lists_luft_skill() {
    let mut registry = SkillRegistry::empty();
    let tool = make_tool();
    let skill = tool.builtin_skill().unwrap();

    registry.add_builtin(
        &skill.name,
        &skill.description,
        &skill.content,
        skill.triggers,
        skill.requires_tools,
    );

    let prompt = registry.available_skills_prompt();
    assert!(prompt.contains("<available_skills>"));
    assert!(prompt.contains("luft-workflow-dsl"));
    assert!(prompt.contains("Lua DSL reference"));
}

#[test]
fn disk_skill_overrides_builtin() {
    let mut registry = SkillRegistry::empty();

    // Simulate a disk-discovered skill with the same name.
    registry.skills.push(skill::discovery::SkillEntry {
        metadata: skill::utils::SkillMetadata {
            name: "luft-workflow-dsl".to_string(),
            description: "User-overridden version".to_string(),
            ..Default::default()
        },
        base_path: std::path::PathBuf::from("/user/skills"),
        skill_file: std::path::PathBuf::from("/user/skills/SKILL.md"),
        source: SkillSource::Project,
        embedded_content: None,
    });

    // Builtin should be no-op now.
    let tool = make_tool();
    let skill = tool.builtin_skill().unwrap();
    registry.add_builtin(
        &skill.name,
        &skill.description,
        &skill.content,
        skill.triggers,
        skill.requires_tools,
    );

    assert_eq!(registry.list().len(), 1);
    let entry = &registry.list()[0];
    assert_eq!(entry.source, SkillSource::Project);
    assert_eq!(entry.metadata.description, "User-overridden version");
}
