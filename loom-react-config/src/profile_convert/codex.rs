use std::path::PathBuf;

use crate::profile::AgentProfile;
use model_spec_core::ModelTier;

use super::{collect_constraints, ExportOutput, role_content};

pub fn convert(profile: &AgentProfile) -> ExportOutput {
    let name = &profile.name;
    let description = profile
        .description
        .as_deref()
        .unwrap_or(&profile.name);

    let mut out = String::new();

    out.push_str(&format!("name = \"{name}\"\n"));
    out.push_str(&format!("description = \"{description}\"\n"));

    if let Some(model_str) = resolve_model(profile) {
        out.push_str(&format!("model = \"{model_str}\"\n"));
    }

    let mut instructions = role_content(profile);
    let constraints = collect_constraints(profile);
    if !constraints.is_empty() {
        instructions.push_str("\n## Constraints\n");
        for c in &constraints {
            instructions.push_str(&format!("- {c}\n"));
        }
    }

    let escaped = escape_toml_value(&instructions);
    out.push_str(&format!("developer_instructions = {escaped}\n"));

    ExportOutput {
        path: PathBuf::from(format!(".codex/agents/{name}.toml")),
        content: out,
    }
}

fn resolve_model(profile: &AgentProfile) -> Option<String> {
    let model = profile.model.as_ref()?;
    if let Some(name) = &model.name {
        return Some(name.clone());
    }
    match model.tier? {
        ModelTier::None => None,
        ModelTier::Light => Some("o3-mini".to_string()),
        ModelTier::Standard => Some("o3".to_string()),
        ModelTier::Strong => Some("o3".to_string()),
        _ => None,
    }
}

fn escape_toml_value(s: &str) -> String {
    if s.contains("\"\"\"") {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else if s.contains('\n') {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"\"\"\n{escaped}\"\"\"")
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}
