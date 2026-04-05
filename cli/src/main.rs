//! Loom CLI binary: run ReAct or DUP agent from the command line.
//!
//! Subcommands: `react` (default ReAct), `dup` (DUP), `tot` (ToT), `got` (GoT), `tool` (list/show tools), `models` (list models).
//! Dispatch lives here; see `args`, `bootstrap`, `display_limits`, `run_flow`, and `subcommands` for implementation.

mod args;
mod bootstrap;
mod display_limits;
mod log_format;
mod logging;
mod output;
mod repl;
mod run_flow;
mod session;
mod subcommands;

pub(crate) use args::Command;

use clap::Parser;

use args::{Args, Command as Cmd, GotArgs};
use bootstrap::{init_logging, print_config_report};
use display_limits::max_reply_len;
use run_flow::{
    build_run_options, output_config, resolve_user_message, run_interactive_mode,
    run_single_turn_mode,
};
use subcommands::{handle_models_command, handle_session_command, handle_tool_command};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    print_config_report();
    let _log_guard = init_logging(&args);

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

    let message = resolve_user_message(&args);
    if !args.interactive && message.is_none() {
        eprintln!("loom: provide a message via -m/--message or positional args");
        std::process::exit(1);
    }

    let cmd = args.cmd.clone().unwrap_or(Cmd::React);
    let got_adaptive = matches!(&cmd, Cmd::Got(GotArgs { got_adaptive: true }));
    let mut opts = build_run_options(&args, message.clone().unwrap_or_default(), got_adaptive);
    let output = output_config(&args);
    let reply_len = max_reply_len();

    if args.interactive {
        run_interactive_mode(&mut opts, &cmd, message, reply_len, &output).await?;
    } else {
        run_single_turn_mode(&mut opts, &cmd, reply_len, &output).await?;
    }
    Ok(())
}
