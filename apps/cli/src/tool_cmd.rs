//! Tool subcommand: list tools and show tool definition.
//!
//! Lists or displays tool specs (name, description, input_schema) from the same
//! tool source used by agent runners. Uses [`build_react_config`](crate::run::build_react_config)
//! and [`build_react_run_context`](anureo::build_react_run_context) so the output
//! matches what would be used for `react`/`dup`/`tot`/`got`.
//!
//! **Interaction**: Called from the `anureo` binary when the user runs `anureo tool list`
//! or `anureo tool show <NAME>`. Uses [`RunOptions`](crate::run::RunOptions) with a
//! placeholder message (not used for execution).

use agent::{build_react_run_context, BuildRunnerError};
use anureo_graph_core::GraphError;
use serde::{Deserialize, Serialize};
use tool_core::ToolSpec;

use crate::run::{build_react_config, RunError};
use agent::RunOptions;

/// Tool show response: either `tool` (JSON) or `tool_yaml` (YAML string).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolShowResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_yaml: Option<String>,
}

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

/// Lists all tools: builds run context from opts, then prints name and description (table or JSON).
///
/// Interacts with [`build_react_config`](crate::run::build_react_config) and
/// [`build_react_run_context`](anureo::build_react_run_context).
pub async fn list_tools(opts: &RunOptions) -> Result<(), RunError> {
    let anureo_opts = opts.clone();
    let (config, _resolved_agent, _) = build_react_config(&anureo_opts);
    // `build_react_config` now registers the workflow tool (and its
    // `workflow` builtin skill) via `RunOptions::default_extra_tools_provider`,
    // so no post-mutation is needed here.
    let ctx = build_react_run_context(&config)
        .await
        .map_err(|e| RunError::Build(BuildRunnerError::Context(e)))?;
    let tools: Vec<ToolSpec> = ctx.tool_source.list_tools().await;
    format_tools_list(&tools, opts.output_json)
}

/// Formats tools list for display.
/// When `output_json` is true, prints a JSON array of tools; otherwise prints a table.
#[allow(clippy::result_large_err)]
pub fn format_tools_list(tools: &[ToolSpec], output_json: bool) -> Result<(), RunError> {
    if output_json {
        let list: Vec<ToolSpecOutput> = tools
            .iter()
            .map(|spec| ToolSpecOutput {
                name: spec.name.clone(),
                description: spec.description.clone().unwrap_or_default(),
                input_schema: spec.input_schema.clone(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
        Ok(())
    } else {
        let table = draw_tools_table(tools);
        println!("{}", table);
        Ok(())
    }
}

/// Draws a table with tool names and descriptions.
/// Descriptions longer than `LIST_DESC_MAX_LEN` are truncated with "...".
fn draw_tools_table(tools: &[ToolSpec]) -> String {
    if tools.is_empty() {
        return "No tools available.".to_string();
    }

    let mut table = String::new();
    table.push_str(&format!("{:<30} {:<60}\n", "Tool", "Description"));
    table.push_str(&format!("{:<30} {:<60}\n", "----", "-----------"));

    for tool in tools {
        let desc = tool.description.as_deref().unwrap_or("");
        let desc = if desc.len() > LIST_DESC_MAX_LEN {
            let cut = desc.floor_char_boundary(LIST_DESC_MAX_LEN);
            format!("{}...", &desc[..cut])
        } else {
            desc.to_string()
        };
        table.push_str(&format!("{:<30} {:<60}\n", tool.name, desc));
    }

    table
}

/// Shows a single tool definition (JSON or YAML).
///
/// **Interaction**: Called from the `anureo` binary when the user runs `anureo tool show <NAME>`.
/// Uses [`build_react_config`](crate::run::build_react_config) and [`build_react_run_context`](anureo::build_react_run_context)
/// to get the tool spec from the context.
pub async fn show_tool(
    name: &str,
    format: ToolShowFormat,
    opts: &RunOptions,
) -> Result<(), RunError> {
    let anureo_opts = opts.clone();
    let (config, _resolved_agent, _) = build_react_config(&anureo_opts);
    // `build_react_config` now registers the workflow tool (and its
    // `workflow` builtin skill) via `RunOptions::default_extra_tools_provider`,
    // so no post-mutation is needed here.
    let ctx = build_react_run_context(&config)
        .await
        .map_err(|e| RunError::Build(BuildRunnerError::Context(e)))?;
    let tools: Vec<ToolSpec> = ctx.tool_source.list_tools().await;

    let spec = tools
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| RunError::ToolNotFound(name.to_string()))?;

    let out = ToolSpecOutput {
        name: spec.name,
        description: spec.description.unwrap_or_default(),
        input_schema: spec.input_schema,
    };

    match format {
        ToolShowFormat::Yaml => {
            let yaml = serde_yaml::to_string(&out)
                .map_err(|e| RunError::Remote(format!("YAML serialization failed: {}", e)))?;
            println!("{}", yaml);
            Ok(())
        }
        ToolShowFormat::Json => {
            let json = serde_json::to_string_pretty(&out)
                .map_err(|e| RunError::Remote(format!("JSON serialization failed: {}", e)))?;
            println!("{}", json);
            Ok(())
        }
    }
}

/// Helper to serialize tool spec for display. Mirrors [`anureo::tool_source::ToolSpec`]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolSpecOutput {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Formats tool show output from ToolShowResponse.
#[allow(clippy::result_large_err)]
pub fn format_tool_show_output(
    r: &ToolShowResponse,
    format: ToolShowFormat,
) -> Result<(), RunError> {
    match format {
        ToolShowFormat::Yaml => {
            if let Some(ref yaml) = r.tool_yaml {
                print!("{}", yaml);
            } else if let Some(ref v) = r.tool {
                let yaml = serde_yaml::to_string(v).map_err(|e| {
                    RunError::Build(BuildRunnerError::Context(GraphError::ExecutionFailed(
                        e.to_string(),
                    )))
                })?;
                print!("{}", yaml);
            } else {
                return Err(RunError::Remote(
                    "no tool or tool_yaml in response".to_string(),
                ));
            }
        }
        ToolShowFormat::Json => {
            if let Some(ref v) = r.tool {
                println!(
                    "{}",
                    serde_json::to_string_pretty(v).map_err(|e| {
                        RunError::Build(BuildRunnerError::Context(GraphError::ExecutionFailed(
                            e.to_string(),
                        )))
                    })?
                );
            } else if let Some(ref yaml) = r.tool_yaml {
                let v: serde_json::Value =
                    serde_yaml::from_str(yaml).map_err(|e| RunError::Remote(e.to_string()))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&v).map_err(|e| {
                        RunError::Build(BuildRunnerError::Context(GraphError::ExecutionFailed(
                            e.to_string(),
                        )))
                    })?
                );
            } else {
                return Err(RunError::Remote(
                    "no tool or tool_yaml in response".to_string(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tools_list_with_empty_vec_shows_message() {
        let empty: Vec<ToolSpec> = vec![];
        // format_tools_list writes to stdout; we just verify it doesn't panic.
        let res = format_tools_list(&empty, false);
        assert!(res.is_ok());
    }

    #[test]
    fn format_tools_list_with_tools_shows_table() {
        let specs = vec![
            ToolSpec {
                name: "test1".to_string(),
                description: Some("Short description".to_string()),
                input_schema: serde_json::Value::Null,
                output_hint: None,
            },
            ToolSpec {
                name: "test2".to_string(),
                description: Some("This is a very long description that should be truncated with ... when displayed in the CLI table to maintain readability and proper table formatting".to_string()),
                input_schema: serde_json::Value::Null,
                output_hint: None,
            },
        ];
        let res = format_tools_list(&specs, false);
        assert!(res.is_ok());
    }

    #[test]
    fn format_tools_list_json_outputs_valid_json() {
        let specs = vec![ToolSpec {
            name: "test1".to_string(),
            description: Some("Short description".to_string()),
            input_schema: serde_json::Value::Null,
            output_hint: None,
        }];
        let res = format_tools_list(&specs, true);
        assert!(res.is_ok());
    }
}
