//! Integration test: prove the full wiring path works:
//! `WorkflowTool::builtin_skill() -> SkillRegistry::add_builtin() -> load_skill_with_dir()`

use skill::discovery::{SkillRegistry, SkillSource};
use tool_core::Tool;
use tool_workflow::WorkflowTool;

fn make_tool() -> WorkflowTool {
    use agent::agent::AgentConfig;
    WorkflowTool::new(AgentConfig::default())
}

#[test]
fn workflow_tool_exposes_builtin_skill() {
    let tool = make_tool();
    let skill = tool
        .builtin_skill()
        .expect("WorkflowTool should expose a builtin skill");
    assert_eq!(skill.name, "workflow");
    assert!(skill.triggers.contains(&"workflow".to_string()));
    assert!(skill.requires_tools.contains(&"workflow".to_string()));
    assert!(skill.content.contains("# Workflow DSL Reference"));
    // References bundled alongside SKILL.md
    assert!(!skill.references.is_empty(), "references should not be empty");
    let ref_names: Vec<&str> = skill.references.iter().map(|(n, _)| n.as_str()).collect();
    assert!(ref_names.iter().any(|n| n.contains("architecture-header")));
    assert!(ref_names.iter().any(|n| n.contains("agent-prompts")));
    assert!(ref_names.iter().any(|n| n.contains("task-decomposition")));
    assert!(ref_names.iter().any(|n| n.contains("adversarial-verification")));
    assert!(ref_names.iter().any(|n| n.contains("examples")));
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
        skill.references.clone(),
    );

    let entries = registry.list();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.source, SkillSource::Builtin);
    assert_eq!(entry.metadata.name, "workflow");
    assert!(entry.embedded_content.is_some());

    // Triggers preserved
    assert!(entry.metadata.triggers.contains(&"workflow".to_string()));
    assert!(entry.metadata.triggers.contains(&"multi-agent".to_string()));

    // References preserved on the entry
    assert!(entry.embedded_files.is_some());
    assert_eq!(
        entry.embedded_files.as_ref().unwrap().len(),
        skill.references.len()
    );
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
        skill.references,
    );

    let (content, _) = registry
        .load_skill_with_dir("workflow")
        .expect("builtin skill should load");

    // Frontmatter stripped, body present
    assert!(content.contains("# Workflow DSL Reference"));
    assert!(content.contains("## Required Structure"));
    assert!(content.contains("agent(opts)"));
    assert!(!content.contains("name: workflow"));

    // References surfaced under "## Additional resources" so the agent can `read` them
    assert!(content.contains("## Additional resources"));
    assert!(content.contains("references/architecture-header.md"));
    assert!(content.contains("references/examples.md"));
}

#[test]
fn available_skills_prompt_lists_workflow_skill() {
    let mut registry = SkillRegistry::empty();
    let tool = make_tool();
    let skill = tool.builtin_skill().unwrap();

    registry.add_builtin(
        &skill.name,
        &skill.description,
        &skill.content,
        skill.triggers,
        skill.requires_tools,
        skill.references,
    );

    let prompt = registry.available_skills_prompt();
    assert!(prompt.contains("<available_skills>"));
    assert!(prompt.contains("workflow"));
    assert!(prompt.contains("Lua DSL reference"));
}

#[test]
fn disk_skill_overrides_builtin() {
    let mut registry = SkillRegistry::empty();

    // Simulate a disk-discovered skill with the same name.
    registry.skills.push(skill::discovery::SkillEntry {
        metadata: skill::utils::SkillMetadata {
            name: "workflow".to_string(),
            description: "User-overridden version".to_string(),
            ..Default::default()
        },
        base_path: std::path::PathBuf::from("/user/skills"),
        skill_file: std::path::PathBuf::from("/user/skills/SKILL.md"),
        source: SkillSource::Project,
        embedded_content: None,
        embedded_files: None,
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
        skill.references,
    );

    assert_eq!(registry.list().len(), 1);
    let entry = &registry.list()[0];
    assert_eq!(entry.source, SkillSource::Project);
    assert_eq!(entry.metadata.description, "User-overridden version");
}