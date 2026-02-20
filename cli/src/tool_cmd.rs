//! Tool subcommand: list tools and show tool definition.
//!
//! Lists or displays tool specs (name, description, input_schema) from the same
//! tool source used by agent runners. Uses [`build_helve_config`](crate::run::build_helve_config)
//! and [`build_react_run_context`](loom::build_react_run_context) so the output
//! matches what would be used for `react`/`dup`/`tot`/`got`.
//!
//! **Interaction**: Called from the `loom` binary when the user runs `loom tool list`
//! or `loom tool show <NAME>`. Uses [`RunOptions`](crate::run::RunOptions) with a
//! placeholder message (not used for execution).

use loom::{
    build_react_run_context, tool_source::ToolSpec, AgentError, BuildRunnerError,
};
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

/// Lists all tools: builds run context from opts, then prints name and description (table or JSON).
///
/// Interacts with [`build_helve_config`](crate::run::build_helve_config) and
/// [`build_react_run_context`](loom::build_react_run_context).
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
    format_tools_list(&tools, opts.output_json)
}

/// Formats tools list for display (used by both local and remote backend).
/// When `output_json` is true, prints a JSON array of tools; otherwise prints a table.
pub fn format_tools_list(tools: &[ToolSpec], output_json: bool) -> Result<(), RunError> {
    if output_json {
        let list: Vec<ToolSpecOutput> = tools
            .iter()
            .map(|s| ToolSpecOutput {
                name: s.name.clone(),
                description: s.description.clone(),
                input_schema: s.input_schema.clone(),
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&list).map_err(|e| {
                RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
                    e.to_string(),
                )))
            })?
        );
        return Ok(());
    }
    if tools.is_empty() {
        println!("NAME\tDESCRIPTION");
        return Ok(());
    }
    let name_width = tools.iter().map(|t| t.name.len()).max().unwrap_or(4).max(4);
    println!("{:<width$}\t{}", "NAME", "DESCRIPTION", width = name_width);
    for spec in tools {
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

/// Formats tool show output from ToolShowResponse (used by remote backend).
pub fn format_tool_show_output(
    r: &loom::ToolShowResponse,
    format: ToolShowFormat,
) -> Result<(), RunError> {
    match format {
        ToolShowFormat::Yaml => {
            if let Some(ref yaml) = r.tool_yaml {
                print!("{}", yaml);
            } else if let Some(ref v) = r.tool {
                let yaml = serde_yaml::to_string(v).map_err(|e| {
                    RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
                        e.to_string(),
                    )))
                })?;
                print!("{}", yaml);
            } else {
                return Err(RunError::Remote("no tool or tool_yaml in response".to_string()));
            }
        }
        ToolShowFormat::Json => {
            if let Some(ref v) = r.tool {
                println!("{}", serde_json::to_string_pretty(v).map_err(|e| {
                    RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
                        e.to_string(),
                    )))
                })?);
            } else if let Some(ref yaml) = r.tool_yaml {
                let v: serde_json::Value =
                    serde_yaml::from_str(yaml).map_err(|e| RunError::Remote(e.to_string()))?;
                println!("{}", serde_json::to_string_pretty(&v).map_err(|e| {
                    RunError::Build(BuildRunnerError::Context(AgentError::ExecutionFailed(
                        e.to_string(),
                    )))
                })?);
            } else {
                return Err(RunError::Remote("no tool or tool_yaml in response".to_string()));
            }
        }
    }
    Ok(())
}

/// Helper to serialize tool spec for display. Mirrors [`loom::tool_source::ToolSpec`]
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

#[cfg(test)]
mod tests {
    use super::*;
    use loom::{ToolShowResponse, ToolSpec};
    use std::path::PathBuf;

    #[test]
    fn format_tools_list_handles_empty_and_json_output() {
        let empty: Vec<ToolSpec> = vec![];
        format_tools_list(&empty, false).unwrap();

        let specs = vec![ToolSpec {
            name: "read_file".to_string(),
            description: Some("Read file content".to_string()),
            input_schema: serde_json::json!({"type":"object"}),
        }];
        format_tools_list(&specs, true).unwrap();
    }

    #[test]
    fn format_tool_show_output_accepts_json_or_yaml_sources() {
        let from_tool = ToolShowResponse {
            id: "1".to_string(),
            tool: Some(serde_json::json!({"name":"read_file","input_schema":{"type":"object"}})),
            tool_yaml: None,
        };
        format_tool_show_output(&from_tool, ToolShowFormat::Json).unwrap();
        format_tool_show_output(&from_tool, ToolShowFormat::Yaml).unwrap();

        let from_yaml = ToolShowResponse {
            id: "2".to_string(),
            tool: None,
            tool_yaml: Some("name: read_file\ninput_schema:\n  type: object\n".to_string()),
        };
        format_tool_show_output(&from_yaml, ToolShowFormat::Yaml).unwrap();
        format_tool_show_output(&from_yaml, ToolShowFormat::Json).unwrap();
    }

    #[test]
    fn format_tool_show_output_errors_when_both_missing() {
        let empty = ToolShowResponse {
            id: "3".to_string(),
            tool: None,
            tool_yaml: None,
        };
        let err = format_tool_show_output(&empty, ToolShowFormat::Json).unwrap_err();
        assert!(err.to_string().contains("no tool or tool_yaml"));
    }

    fn invalid_opts() -> RunOptions {
        RunOptions {
            message: String::new(),
            working_folder: Some(PathBuf::from(
                "/definitely/not/exist/loom-cli-tool-cmd-tests",
            )),
            thread_id: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 100,
            output_json: true,
        }
    }

    #[tokio::test]
    async fn list_tools_returns_error_for_invalid_context() {
        let res = list_tools(&invalid_opts()).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn show_tool_returns_error_for_invalid_context() {
        let res = show_tool(&invalid_opts(), "read_file", ToolShowFormat::Json).await;
        assert!(res.is_err());
    }
}
