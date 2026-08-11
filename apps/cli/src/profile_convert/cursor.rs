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

    if let Some(model) = resolve_model(profile) {
        fm.push_str(&format!("model: {model}\n"));
    }

    fm.push_str("---\n");

    let body = role_content(profile);
    let constraints = collect_constraints(profile);
    let content = format!("{fm}\n{body}{}", format_constraints_section(&constraints));

    ExportOutput {
        path: PathBuf::from(format!(".cursor/agents/{name}.md")),
        content,
    }
}

fn resolve_model(profile: &AgentProfile) -> Option<&'static str> {
    let model = profile.model.as_ref()?;
    if model.name.is_some() {
        return Some("inherit");
    }
    match model.tier? {
        ModelTier::None => None,
        ModelTier::Light => Some("fast"),
        ModelTier::Standard => Some("inherit"),
        ModelTier::Strong => Some("inherit"),
        _ => None,
    }
}
