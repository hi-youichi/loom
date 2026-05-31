//! Loom CLI binary: run ReAct or DUP agent from the command line.
//!
//! Subcommands: `react` (default ReAct), `dup` (DUP), `tot` (ToT), `got` (GoT), `tool` (list/show tools), `models` (list models), `mcp` (manage MCP servers).
//! Dispatch lives here; see `args`, `bootstrap`, `display_limits`, `run_flow`, and `subcommands` for implementation.

mod args;
mod bootstrap;
mod codex_event_builder;
mod display_limits;
mod goal_cmd;
mod logging;
mod mcp_manager;
mod output;
mod repl;
mod review_history;
mod review_cmd;
mod review_skill_cmd;
mod run_flow;
mod session;
mod subcommands;
mod task_cmd;
mod task_db;

pub(crate) use args::Command;

use clap::Parser;

use args::{Args, Command as Cmd, GotArgs};
use bootstrap::{init_logging, preserve_shell_env, print_config_report};
use display_limits::max_reply_len;
use loom::cli_run::RunCancellation;
use run_flow::{
    build_run_options, output_config, resolve_user_message, run_interactive_mode,
    run_single_turn_mode,
};
use cli::run::background_review::wait_for_pending_reviews;
use subcommands::{
    handle_agent_command, handle_curator_command, handle_mcp_command,
    handle_memory_command, handle_models_command, handle_session_command, handle_skills_command,
    handle_tool_command,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::parse();

    // Preserve shell environment variables BEFORE config.toml is loaded.
    // This allows us to distinguish between shell-set and config.toml-set LOG_FILE.
    let shell_env = preserve_shell_env();

    print_config_report();

    if let Some(Cmd::Serve(_)) = &args.cmd {
        if args.log_file.is_none() && std::env::var_os("LOG_FILE").is_none() {
            // Use the same default as CLI logging
            let log_dir = config::home::cli_logs_dir();
            let _ = std::fs::create_dir_all(&log_dir);
            let log_path = log_dir.join("loom-serve.log");
            eprintln!("config: log_file={}", log_path.display());
            args.log_file = Some(log_path);
        }
    }

    let _log_guard = init_logging(&args, shell_env);

    if let Some(Cmd::Serve(sa)) = &args.cmd {
        if let Err(e) = serve::run_serve(sa.addr.as_deref(), false).await {
            eprintln!("serve error: {}", e);
            let msg = e.to_string();
            if msg.contains("Address already in use") || msg.contains("already in use") {
                eprintln!(
                    "hint: 端口已被占用。可尝试：1) 使用 --addr 指定其他地址，如 --addr 127.0.0.1:8081；2) 结束占用该端口的进程（如 lsof -i :8080）。"
                );
            }
            std::process::exit(1);
        }
        return Ok(());
    }

    if let Some(Cmd::Session(sa)) = &args.cmd {
        handle_session_command(sa, args.json).await?;
        return Ok(());
    }
    if let Some(Cmd::Tool(ta)) = &args.cmd {
        if let Err(err) = handle_tool_command(&args, ta).await {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::Models(ma)) = &args.cmd {
        if let Err(err) = handle_models_command(&args, ma).await {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::Mcp(ma)) = &args.cmd {
        if let Err(err) = handle_mcp_command(ma, args.json) {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::Agent(aa)) = &args.cmd {
        if let Err(err) = handle_agent_command(aa) {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::Goal(ga)) = &args.cmd {
        goal_cmd::handle_goal_command(ga).await?;
        return Ok(());
    }
    if let Some(Cmd::Skills(sa)) = &args.cmd {
        if let Err(err) = handle_skills_command(sa, args.json) {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::Curator(ca)) = &args.cmd {
        if let Err(err) = handle_curator_command(ca, args.json) {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::Memory(ma)) = &args.cmd {
        if let Err(err) = handle_memory_command(ma, args.json) {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::ReviewSkill(ra)) = &args.cmd {
        if let Err(err) = review_skill_cmd::handle_review_skill_command(ra).await {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::Review(ra)) = &args.cmd {
        if let Err(err) = review_cmd::handle_review_command(ra, args.json).await {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(Cmd::Task(ta)) = &args.cmd {
        if let Err(err) = task_cmd::handle_task_command(ta).await {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return Ok(());
    }

    let message = resolve_user_message(&args);
    if !args.interactive && message.is_none() {
        eprintln!("loom: provide a message via -m/--message or positional args");
        std::process::exit(1);
    }

    let cmd = args.cmd.clone().unwrap_or(Cmd::React);
    let got_adaptive = matches!(&cmd, Cmd::Got(GotArgs { got_adaptive: true }));
    let run_cancellation = RunCancellation::new(0);
    let rc_clone = run_cancellation.clone();
    ctrlc::set_handler(move || {
        rc_clone.cancel();
    })?;

    let mut opts = build_run_options(&args, message.clone().unwrap_or_default(), got_adaptive);
    opts.cancellation = Some(run_cancellation);
    let output = output_config(&args);
    let reply_len = max_reply_len();

    if args.interactive {
        run_interactive_mode(&mut opts, &cmd, message, reply_len, &output).await?;
    } else {
        run_single_turn_mode(&mut opts, &cmd, reply_len, &output).await?;
    }

    // Wait for all pending background reviews to complete before exiting.
    // This ensures that memory updates and skill modifications are persisted.
    let _ = wait_for_pending_reviews().await;

    Ok(())
}
