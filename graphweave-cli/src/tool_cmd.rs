//! Tool subcommand: list tools and show tool definition.
//!
//! Lists or displays tool specs (name, description, input_schema) from the same
//! tool source used by agent runners. Uses [`build_helve_config`](crate::run::build_helve_config)
//! and [`build_react_run_context`](graphweave::build_react_run_context) so the output
//! matches what would be used for `react`/`dup`/`tot`/`got`.
//!
//! **Interaction**: Called from the `graphweave` binary when the user runs `graphweave tool list`
//! or `graphweave tool show <NAME>`. Uses [`RunOptions`](crate::run::RunOptions) with a
//! placeholder message (not used for execution).

use graphweave::{build_react_run_context, AgentError, BuildRunnerError};
use serde::Serialize;

use crate::run::{build_helve_config, RunError, RunOptions};

/// Maximum length for description in the list table. Longer descriptions are truncated with "...".
const LIST_DESC_MAX_LEN: usize = 60;

/// Output format for `tool show`: YAML (human-readable) or JSON (machine-readable).
///
/// **Interaction**: Passed to [`show_tool`] to choose serialization format.
#[derive(Debug, Clone, Copy, Default)]
pub enum ToolShowFormat {
    #[default]
    Yaml,
    Json,
}

/// Lists all tools: builds run context from opts, then prints name and description in a table.
///
/// Interacts with [`build_helve_config`](crate::run::build_helve_config) and
/// [`build_react_run_context`](graphweave::build_react_run_context).
pub async fn list_tools(opts: &RunOptions) -> Result<(), RunError> {
    let (_helve, config) = build_helve_config(opts);
    let ctx = build_react_run_context(&config)
        .await
        .map_err(|e| RunError::Build(BuildRunnerError::Context(e)))?;
    let tools = ctx.tool_source.list_tools().await.map_err(|e| {
        RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
            e.to_string(),
        )))
    })?;

    if tools.is_empty() {
        println!("NAME\tDESCRIPTION");
        return Ok(());
    }

    let name_width = tools.iter().map(|t| t.name.len()).max().unwrap_or(4).max(4);
    let header_name = "NAME";
    let header_desc = "DESCRIPTION";
    println!("{:<width$}\t{}", header_name, header_desc, width = name_width);
    for spec in &tools {
        let desc = spec
            .description
            .as_deref()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("");
        let desc = if desc.len() > LIST_DESC_MAX_LEN {
            format!("{}...", &desc[..LIST_DESC_MAX_LEN])
        } else {
            desc.to_string()
        };
        println!("{:<width$}\t{}", spec.name, desc, width = name_width);
    }
    Ok(())
}

/// Helper to serialize tool spec for display. Mirrors [`graphweave::tool_source::ToolSpec`]
/// with the same field types so that YAML/JSON output is clean (input_schema as object).
#[derive(Serialize)]
struct ToolSpecOutput {
    name: String,
    description: Option<String>,
    input_schema: serde_json::Value,
}

/// Shows one tool by name: builds run context, finds the tool, prints full spec in YAML or JSON.
///
/// Returns [`RunError::ToolNotFound`](crate::run::RunError::ToolNotFound) if name is not in the list.
pub async fn show_tool(opts: &RunOptions, name: &str, format: ToolShowFormat) -> Result<(), RunError> {
    let (_helve, config) = build_helve_config(opts);
    let ctx = build_react_run_context(&config)
        .await
        .map_err(|e| RunError::Build(BuildRunnerError::Context(e)))?;
    let tools = ctx.tool_source.list_tools().await.map_err(|e| {
        RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
            e.to_string(),
        )))
    })?;

    let spec = tools
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| RunError::ToolNotFound(name.to_string()))?;

    let out = ToolSpecOutput {
        name: spec.name,
        description: spec.description,
        input_schema: spec.input_schema,
    };

    match format {
        ToolShowFormat::Yaml => {
            let yaml = serde_yaml::to_string(&out).map_err(|e| {
                RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
                    e.to_string(),
                )))
            })?;
            print!("{}", yaml);
        }
        ToolShowFormat::Json => {
            let json = serde_json::to_string_pretty(&out).map_err(|e| {
                RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
                    e.to_string(),
                )))
            })?;
            println!("{}", json);
        }
    }
    Ok(())
}
