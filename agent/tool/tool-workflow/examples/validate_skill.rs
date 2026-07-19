//! Runtime smoke test: prove the builtin skill wiring works end-to-end
//! in an actual binary, mirroring what the agent init code does.
//!
//! Run with: `cargo run -p tool-workflow --example validate_skill`

use agent::agent::AgentConfig;
use skill::discovery::{SkillRegistry, SkillSource};
use std::sync::Arc;
use tool_core::Tool;
use tool_workflow::{WorkflowRuntime, WorkflowStartTool};

fn main() {
    println!("=== Workflow builtin skill: runtime validation ===\n");

    // 1. WorkflowStartTool exposes the builtin skill
    let tool = WorkflowStartTool::new(Arc::new(WorkflowRuntime::new(AgentConfig::default())));
    let skill = tool
        .builtin_skill()
        .expect("WorkflowStartTool must expose builtin_skill()");
    println!("[1] WorkflowStartTool::builtin_skill()");
    println!("    name:         {}", skill.name);
    println!("    description:  {}", skill.description);
    println!("    triggers:     {:?}", skill.triggers);
    println!("    requires:     {:?}", skill.requires_tools);
    println!(
        "    content size: {} bytes / ~{} tokens",
        skill.content.len(),
        skill.content.len() / 4
    );
    println!("    references:   {} bundled files", skill.references.len());
    for (name, _content) in &skill.references {
        println!("                  - {}", name);
    }
    println!();

    // 2. Empty registry (mirrors fresh agent start)
    let mut registry = SkillRegistry::empty();
    let empty_count = registry.list().len();
    println!("[2] Empty SkillRegistry: {} skills", empty_count);

    // 3. Simulate agent init: inject_builtin_skills(extra_tools)
    let extra_tools: Vec<Arc<dyn Tool>> = vec![Arc::new(tool)];
    for t in &extra_tools {
        if let Some(s) = t.builtin_skill() {
            registry.add_builtin(
                &s.name,
                &s.description,
                &s.content,
                s.triggers.clone(),
                s.requires_tools.clone(),
                s.references.clone(),
            );
        }
    }
    let after_count = registry.list().len();
    println!(
        "[3] After inject_builtin_skills: {} skills (delta = +{})\n",
        after_count,
        after_count - empty_count
    );

    // 4. Inspect the injected entry (clone the data we need to release the borrow)
    let (entry_source, entry_name, embedded_size, requires_tools, embedded_files_count) = {
        let e = &registry.list()[0];
        (
            e.source,
            e.metadata.name.clone(),
            e.embedded_content.as_ref().unwrap().len(),
            e.metadata
                .metadata
                .as_ref()
                .map(|b| b.conditions.requires_tools.clone()),
            e.embedded_files.as_ref().map(|f| f.len()).unwrap_or(0),
        )
    };
    println!("[4] Injected SkillEntry");
    println!("    source:        {:?}", entry_source);
    println!("    name:          {}", entry_name);
    println!("    embedded:      {} bytes", embedded_size);
    println!("    embedded_files: {} references", embedded_files_count);
    if let Some(rt) = requires_tools {
        println!("    requires_tools (frontmatter round-trip): {:?}", rt);
    }

    // 5. load_skill_with_dir works (mirrors what `skill` tool does at runtime)
    let (content, _base) = registry
        .load_skill_with_dir("workflow")
        .expect("builtin skill must load");
    let content_len = content.len();
    let has_when = content.contains("## 1 When to use which tool");
    let has_primitives = content.contains("function main()") && content.contains("agent(");
    let has_execution_model = content.contains("## 2 Execution model");
    let has_additional_resources = content.contains("## Additional resources");
    let has_examples_ref = content.contains("references/examples.md");
    println!("\n[5] load_skill_with_dir(\"workflow\")");
    println!("    body size:                    {} bytes", content_len);
    println!("    has 'When to use which tool':  {}", has_when);
    println!("    has 'main()' + 'agent(':       {}", has_primitives);
    println!("    has 'Execution model':         {}", has_execution_model);
    println!(
        "    has '## Additional resources': {}",
        has_additional_resources
    );
    println!("    lists 'references/examples.md': {}", has_examples_ref);

    // 6. available_skills_prompt includes it (drives nudge system)
    let prompt = registry.available_skills_prompt();
    let in_prompt = prompt.contains("workflow");
    println!("\n[6] available_skills_prompt");
    println!("    contains 'workflow': {}", in_prompt);
    println!("    --- prompt excerpt ---");
    for line in prompt.lines().take(8) {
        println!("    {}", line);
    }

    // 7. User override works (disk > builtin).
    registry.skills.insert(
        0,
        skill::discovery::SkillEntry {
            metadata: skill::utils::SkillMetadata {
                name: "workflow".to_string(),
                description: "USER-OVERRIDDEN".to_string(),
                ..Default::default()
            },
            base_path: Default::default(),
            skill_file: Default::default(),
            source: SkillSource::Project,
            embedded_content: None,
            embedded_files: None,
        },
    );
    let entries_before = registry.list().len();
    let tool2 = WorkflowStartTool::new(Arc::new(WorkflowRuntime::new(AgentConfig::default())));
    if let Some(s) = tool2.builtin_skill() {
        registry.add_builtin(
            &s.name,
            &s.description,
            &s.content,
            s.triggers,
            s.requires_tools,
            s.references,
        );
    }
    let entries_after = registry.list().len();
    let add_builtin_was_noop = entries_after == entries_before;

    let override_entry = registry.list().first().expect("user override at index 0");
    let (final_source, final_desc) = (
        override_entry.source,
        override_entry.metadata.description.clone(),
    );

    println!("\n[7] User override takes precedence");
    println!("    user entry source: {:?} (expect Project)", final_source);
    println!("    user entry desc:   {}", final_desc);
    println!(
        "    add_builtin no-op: {} (expect true when same name exists)",
        add_builtin_was_noop
    );
    println!(
        "    entries before/after add_builtin: {} / {}",
        entries_before, entries_after
    );

    // 8. apply_toolset_filters: hides when workflow_start tool missing
    let mut registry2 = SkillRegistry::empty();
    if let Some(s) = WorkflowStartTool::new(Arc::new(WorkflowRuntime::new(AgentConfig::default()))).builtin_skill() {
        registry2.add_builtin(
            &s.name,
            &s.description,
            &s.content,
            s.triggers,
            s.requires_tools,
            s.references,
        );
    }
    let available = std::collections::HashSet::new(); // empty: no tools
    registry2.apply_toolset_filters(Some(&available), None);
    let filtered_count = registry2.list().len();
    println!("\n[8] apply_toolset_filters (no tools available)");
    println!(
        "    builtin skill hidden: {} (expect true since 'workflow_start' missing)",
        filtered_count == 0
    );

    // Final verdict
    println!("\n=== VERDICT ===");
    let all_ok = matches!(entry_source, SkillSource::Builtin)
        && content_len > 0
        && has_when
        && has_primitives
        && has_execution_model
        && has_additional_resources
        && has_examples_ref
        && in_prompt
        && matches!(final_source, SkillSource::Project)
        && filtered_count == 0;
    if all_ok {
        println!("PASS — builtin skill wiring is correct end-to-end.");
        std::process::exit(0);
    } else {
        println!("FAIL — see checks above.");
        std::process::exit(1);
    }
}
