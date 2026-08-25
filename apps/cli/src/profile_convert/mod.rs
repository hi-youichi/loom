mod claude_code;
mod codex;
mod cursor;
pub mod error;

use std::path::PathBuf;

use agent::profile::{resolve_profile, AgentProfile};

pub use error::ConvertError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    ClaudeCode,
    Codex,
    Cursor,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 3] = [
        ExportFormat::ClaudeCode,
        ExportFormat::Codex,
        ExportFormat::Cursor,
    ];
}

impl std::str::FromStr for ExportFormat {
    type Err = ConvertError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude-code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "cursor" => Ok(Self::Cursor),
            _ => Err(ConvertError::UnknownFormat(s.to_string())),
        }
    }
}

pub struct ExportOutput {
    pub path: PathBuf,
    pub content: String,
}

pub fn export(name: &str, format: ExportFormat) -> Result<ExportOutput, ConvertError> {
    let profile = resolve_profile(name)?;
    match format {
        ExportFormat::ClaudeCode => Ok(claude_code::convert(&profile)),
        ExportFormat::Codex => Ok(codex::convert(&profile)),
        ExportFormat::Cursor => Ok(cursor::convert(&profile)),
    }
}

pub fn export_all(name: &str) -> Result<Vec<ExportOutput>, ConvertError> {
    let profile = resolve_profile(name)?;
    Ok(vec![
        claude_code::convert(&profile),
        codex::convert(&profile),
        cursor::convert(&profile),
    ])
}

fn role_content(profile: &AgentProfile) -> String {
    profile
        .role
        .as_ref()
        .and_then(|r| r.content.clone())
        .unwrap_or_default()
}

fn collect_constraints(profile: &AgentProfile) -> Vec<String> {
    let mut constraints = Vec::new();

    if let Some(tools) = &profile.tools {
        if let Some(builtin) = &tools.builtin {
            if let Some(disabled) = &builtin.disabled {
                for tool in disabled {
                    constraints.push(format!("Do NOT use {tool} tool"));
                }
            }
            if let Some(enabled) = &builtin.enabled {
                constraints.push(format!("Only use these tools: {}", enabled.join(", ")));
            }
        }
    }

    if let Some(behavior) = &profile.behavior {
        if let Some(max_iter) = behavior.max_iterations {
            constraints.push(format!("Maximum {max_iter} iterations"));
        }
        if let Some(timeout) = behavior.timeout {
            constraints.push(format!("Timeout after {timeout}s"));
        }
    }

    constraints
}

fn format_constraints_section(constraints: &[String]) -> String {
    if constraints.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n## Constraints\n");
    for c in constraints {
        s.push_str("- ");
        s.push_str(c);
        s.push('\n');
    }
    s
}
