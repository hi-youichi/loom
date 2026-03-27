//! Handlers for `tool`, `models`, and `session` CLI subcommands.

use cli::{cli_list_models, cli_list_tools, cli_show_tool, ToolShowFormat};

use crate::args::{Args, ModelsArgs, ModelsCommand, ToolArgs, ToolCommand};
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
        ModelsCommand::Show(show_args) => {
            cli_list_models(&opts, Some(&show_args.name)).await?
        }
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
