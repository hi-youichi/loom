//! Build run options and execute single-turn or interactive agent runs.

use cli::RunOptions;

use crate::args::{Args, Command};
use crate::display_limits::{generate_session_id, max_message_len};
use crate::output::{emit_run_output, make_stream_out, OutputConfig};
use crate::repl::{run_one_turn, run_repl_loop};
use loom::UserContent;

pub(crate) fn resolve_user_message(args: &Args) -> Option<String> {
    args.message.clone().or_else(|| {
        if args.rest.is_empty() {
            None
        } else {
            Some(args.rest.join(" "))
        }
    })
}

pub(crate) fn output_config(args: &Args) -> OutputConfig {
    OutputConfig {
        json: args.json,
        pretty: args.pretty,
        file: args.file.clone(),
    }
}

pub(crate) fn build_run_options(args: &Args, message: String, got_adaptive: bool) -> RunOptions {
    RunOptions {
        message: UserContent::Text(message),
        working_folder: args.working_folder.clone(),
        session_id: None,
        cancellation: None,
        thread_id: args.session_id.clone(),
        agent: args.agent.clone(),
        verbose: args.verbose,
        got_adaptive,
        display_max_len: max_message_len(),
        output_json: args.json,
        model: args.model.clone(),
        mcp_config_path: args.mcp_config.clone(),
        output_timestamp: args.timestamp,
        dry_run: args.dry,
        provider: args.provider.clone(),
        base_url: None,
        api_key: None,
        provider_type: None,
    }
}

fn print_session_status(session_id: Option<&str>, ended: bool, json: bool) {
    if json {
        return;
    }
    if let Some(session_id) = session_id {
        if ended {
            eprintln!("Session ended: {}", session_id);
        } else {
            eprintln!("Session: {}", session_id);
        }
    }
}

fn ensure_session_id(opts: &mut RunOptions) {
    if opts.thread_id.is_none() {
        opts.thread_id = Some(generate_session_id());
    }
}

pub(crate) async fn run_single_turn_mode(
    opts: &mut RunOptions,
    cmd: &Command,
    reply_len: usize,
    output: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_session_id(opts);
    print_session_status(opts.thread_id.as_deref(), false, output.json);
    let output_value = run_one_turn(opts, cmd, make_stream_out(output)).await?;
    emit_run_output(
        output_value,
        output,
        opts.thread_id.as_deref(),
        reply_len,
        opts.output_timestamp,
    )?;
    print_session_status(opts.thread_id.as_deref(), true, output.json);
    Ok(())
}

pub(crate) async fn run_interactive_mode(
    opts: &mut RunOptions,
    cmd: &Command,
    initial_message: Option<String>,
    reply_len: usize,
    output: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_session_id(opts);
    print_session_status(opts.thread_id.as_deref(), false, output.json);

    let stream_out = make_stream_out(output);
    if let Some(msg) = initial_message.filter(|msg| !msg.trim().is_empty()) {
        opts.message = UserContent::Text(msg);
        match run_one_turn(opts, cmd, stream_out.clone()).await {
            Ok(output_value) => emit_run_output(
                output_value,
                output,
                opts.thread_id.as_deref(),
                reply_len,
                opts.output_timestamp,
            )?,
            Err(err) => {
                eprintln!("error: {}", err);
                std::process::exit(1);
            }
        }
    }

    run_repl_loop(opts, cmd, reply_len, output.clone(), stream_out).await?;
    print_session_status(opts.thread_id.as_deref(), true, output.json);
    println!("Bye.");
    Ok(())
}
