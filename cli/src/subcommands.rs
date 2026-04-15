//! Handlers for `tool`, `models`, `session`, and `mcp` CLI subcommands.

use cli::{cli_list_models, cli_list_tools, cli_show_tool, ToolShowFormat};

use crate::args::{Args, McpArgs, McpCommand, ModelsArgs, ModelsCommand, ToolArgs, ToolCommand};
use crate::mcp_manager::{AddMcpArgs, EditMcpArgs, McpManager, ServerDetail, ServerInfo};
use crate::run_flow::build_run_options;
use crate::session::{SessionArgs, SessionCommand, SessionManager};

pub(crate) async fn handle_tool_command(
    args: &Args,
    tool_args: &ToolArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = build_run_options(args, String::new(), false);
    match &tool_args.sub {
        ToolCommand::List => cli_list_tools(&opts).await?,
        ToolCommand::Show(show_args) => {
            let format = if args.json || show_args.output.eq_ignore_ascii_case("json") {
                ToolShowFormat::Json
            } else {
                ToolShowFormat::Yaml
            };
            cli_show_tool(&opts, &show_args.name, format).await?;
        }
    }
    Ok(())
}

pub(crate) async fn handle_models_command(
    args: &Args,
    models_args: &ModelsArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = build_run_options(args, String::new(), false);
    match &models_args.sub {
        ModelsCommand::List => cli_list_models(&opts, None).await?,
        ModelsCommand::Show(show_args) => cli_list_models(&opts, Some(&show_args.name)).await?,
    }
    Ok(())
}

pub(crate) async fn handle_session_command(
    sa: &SessionArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = SessionManager::with_default_path();

    match &sa.command {
        SessionCommand::List => {
            let sessions = manager.list_sessions()?;
            manager.print_session_list(&sessions, json)?;
        }
        SessionCommand::Show { session_id } => match manager.show_session(session_id)? {
            Some(detail) => manager.print_session_detail(&detail, json)?,
            None => {
                eprintln!("Session not found: {}", session_id);
                std::process::exit(1);
            }
        },
        SessionCommand::Delete { session_id } => {
            let count = manager.delete_session(session_id)?;
            if json {
                let result = serde_json::json!({
                    "session_id": session_id,
                    "deleted_checkpoints": count
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Deleted session {} ({} checkpoints)", session_id, count);
            }
        }
    }
    Ok(())
}

pub(crate) fn handle_mcp_command(
    mcp_args: &McpArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = McpManager::new()?;

    match &mcp_args.command {
        McpCommand::List => {
            let servers = manager.list_servers()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&servers)?);
            } else {
                print_server_list(&servers);
            }
        }
        McpCommand::Show { name } => match manager.show_server(name)? {
            Some(detail) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&detail)?);
                } else {
                    print_server_detail(&detail);
                }
            }
            None => {
                eprintln!("MCP server not found: {}", name);
                std::process::exit(1);
            }
        },
        McpCommand::Add(add_args) => {
            let cli_args = AddMcpArgs {
                name: add_args.name.clone(),
                command: add_args.command.clone(),
                args: add_args.args.clone(),
                url: add_args.url.clone(),
                env: add_args.env.clone(),
                disabled: add_args.disabled,
            };
            manager.add_server(&cli_args)?;
            println!("MCP server '{}' added successfully", add_args.name);
        }
        McpCommand::Edit(edit_args) => {
            let cli_args = EditMcpArgs {
                command: edit_args.command.clone(),
                args: edit_args.args.clone(),
                url: edit_args.url.clone(),
                env: edit_args.env.clone(),
                disabled: edit_args.disabled,
            };
            manager.edit_server(&edit_args.name, &cli_args)?;
            println!("MCP server '{}' updated successfully", edit_args.name);
        }
        McpCommand::Delete { name } => {
            if manager.delete_server(name)? {
                println!("MCP server '{}' deleted successfully", name);
            } else {
                eprintln!("MCP server not found: {}", name);
                std::process::exit(1);
            }
        }
        McpCommand::Enable { name } => {
            manager.enable_server(name)?;
            println!("MCP server '{}' enabled successfully", name);
        }
        McpCommand::Disable { name } => {
            manager.disable_server(name)?;
            println!("MCP server '{}' disabled successfully", name);
        }
    }
    Ok(())
}

fn print_server_list(servers: &[ServerInfo]) {
    println!("MCP Servers:");
    println!("{}", "─".repeat(80));

    if servers.is_empty() {
        println!("No MCP servers configured.");
        return;
    }

    for server in servers {
        let status = if server.disabled { "[disabled]" } else { "" };
        println!("  • {} {}", server.name, status);
        println!("    Type: {}", server.server_type);
        if let Some(cmd) = &server.command {
            println!("    Command: {}", cmd);
        }
        if let Some(url) = &server.url {
            println!("    URL: {}", url);
        }
        println!();
    }
}

fn print_server_detail(detail: &ServerDetail) {
    println!("MCP Server: {}", detail.name);
    println!("{}", "═".repeat(80));

    let status = if detail.entry.disabled {
        "disabled"
    } else {
        "enabled"
    };
    println!("Status: {}", status);

    if let Some(cmd) = &detail.entry.command {
        println!("Command: {}", cmd);
        if !detail.entry.args.is_empty() {
            println!("Args: {}", detail.entry.args.join(" "));
        }
    }

    if let Some(url) = &detail.entry.url {
        println!("URL: {}", url);
    }

    if !detail.entry.env.is_empty() {
        println!("Environment:");
        for (key, value) in &detail.entry.env {
            let masked_value = config::mask_value(value);
            println!("  {}={}", key, masked_value);
        }
    }

    if !detail.entry.headers.is_empty() {
        println!("Headers:");
        for (key, value) in &detail.entry.headers {
            println!("  {}: {}", key, value);
        }
    }
}
