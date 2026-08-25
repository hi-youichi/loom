use std::path::PathBuf;

use agent::profile::AgentProfile;
use model_spec_core::ModelTier;

use super::{collect_constraints, format_constraints_section, role_content, ExportOutput};

pub fn convert(profile: &AgentProfile) -> ExportOutput {
    let name = &profile.name;
    let description = profile.description.as_deref().unwrap_or(&profile.name);

    let mut fm = String::from("---\n");
    fm.push_str(&format!("name: {name}\n"));
    fm.push_str(&format!("description: \"{description}\"\n"));

    if let Some(model_str) = resolve_model(profile) {
        fm.push_str(&format!("model: {model_str}\n"));
    }
    if let Some(tools_str) = resolve_tools_field(profile) {
        fm.push_str(&format!("tools: {tools_str}\n"));
    }
    if let Some(dis_str) = resolve_disallowed_tools(profile) {
        fm.push_str(&format!("disallowedTools: {dis_str}\n"));
    }
    if let Some(max_turns) = resolve_max_turns(profile) {
        fm.push_str(&format!("maxTurns: {max_turns}\n"));
    }
    fm.push_str("---\n");

    let mut body = role_content(profile);
    let constraints = collect_constraints(profile);
    body.push_str(&format_constraints_section(&constraints));

    let content = format!("{fm}\n{body}");

    ExportOutput {
        path: PathBuf::from(format!(".claude/agents/{name}.md")),
        content,
    }
}

fn resolve_model(profile: &AgentProfile) -> Option<String> {
    let model = profile.model.as_ref()?;
    if let Some(name) = &model.name {
        return Some(name.clone());
    }
    match model.tier? {
        ModelTier::None => None,
        ModelTier::Light => Some("haiku".to_string()),
        ModelTier::Standard => Some("sonnet".to_string()),
        ModelTier::Strong => Some("opus".to_string()),
        _ => None,
    }
}

fn resolve_tools_field(profile: &AgentProfile) -> Option<String> {
    let enabled = profile.tools.as_ref()?.builtin.as_ref()?.enabled.as_ref()?;

    let mapped: Vec<&str> = enabled
        .iter()
        .filter_map(|t| anureo_to_claude_tool(t))
        .collect();
    if mapped.is_empty() {
        return None;
    }
    Some(mapped.join(", "))
}

fn resolve_disallowed_tools(profile: &AgentProfile) -> Option<String> {
    let disabled = profile
        .tools
        .as_ref()?
        .builtin
        .as_ref()?
        .disabled
        .as_ref()?;

    let mapped: Vec<&str> = disabled
        .iter()
        .filter_map(|t| anureo_to_claude_tool(t))
        .collect();
    if mapped.is_empty() {
        return None;
    }
    Some(mapped.join(", "))
}

fn resolve_max_turns(profile: &AgentProfile) -> Option<u32> {
    profile.behavior.as_ref()?.max_iterations
}

fn anureo_to_claude_tool(name: &str) -> Option<&'static str> {
    Some(match name {
        "bash" => "Bash",
        "read" | "read_file" => "Read",
        "write" | "write_file" => "Write",
        "edit" | "edit_file" | "multiedit" | "apply_patch" => "Edit",
        "glob" | "ls" => "Glob",
        "grep" => "Grep",
        "web_fetcher" => "WebFetch",
        "delete_file" => "Edit",
        "create_dir" | "move_file" => "Bash",
        _ => return None,
    })
}
