//! Loom CLI binary: run ReAct or DUP agent from the command line.
//!
//! Subcommands: `react` (default ReAct), `dup` (DUP), `tot` (ToT), `got` (GoT), `tool` (list/show tools).

mod log_format;
mod logging;
mod repl;

use clap::{Parser, Subcommand};
use cli::{
    LocalBackend, RemoteBackend, RunBackend, RunOutput, RunOptions, StreamOut, ToolShowFormat,
};
use repl::{run_one_turn, run_repl_loop};
use std::path::PathBuf;
use std::sync::Arc;

const DEFAULT_WS_URL: &str = "ws://127.0.0.1:8080";

#[derive(Parser, Debug)]
#[command(name = "loom")]
#[command(about = "Loom — run ReAct or DUP agent from CLI")]
struct Args {
    #[command(subcommand)]
    cmd: Option<Command>,

    /// User message (or pass as first positional argument)
    #[arg(short, long, value_name = "TEXT")]
    message: Option<String>,

    /// Positional args: user message when -m/--message is not used
    #[arg(trailing_var_arg = true)]
    rest: Vec<String>,

    /// Working folder (for file tools); default: /tmp when not set
    #[arg(short, long, value_name = "DIR")]
    working_folder: Option<PathBuf>,

    /// Thread ID for conversation continuity (checkpointer)
    #[arg(long, value_name = "ID")]
    thread_id: Option<String>,

    /// Verbose: log node enter/exit and graph execution
    #[arg(short, long)]
    verbose: bool,

    /// Interactive REPL: after output, prompt for input and continue conversation
    #[arg(short, long)]
    interactive: bool,

    /// Use local execution (default: remote)
    #[arg(long)]
    local: bool,

    /// Remote WebSocket URL (default: ws://127.0.0.1:8080 or LOOM_REMOTE_URL)
    #[arg(long, value_name = "URL")]
    remote: Option<String>,

    /// Do not auto-start server when remote connection fails
    #[arg(long)]
    no_auto_start: bool,

    /// Output all data as JSON (stream events + reply for agent run; JSON array for tool list; JSON for tool show)
    #[arg(long)]
    json: bool,

    /// When using --json, write output to this file instead of stdout
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,

    /// When using --json, pretty-print (multi-line). Default: compact, one line per event
    #[arg(long)]
    pretty: bool,
}

fn resolve_remote_url(args: &Args) -> String {
    if let Some(ref u) = args.remote {
        return u.clone();
    }
    std::env::var("LOOM_REMOTE_URL").unwrap_or_else(|_| DEFAULT_WS_URL.to_string())
}

fn resolve_auto_start(args: &Args) -> bool {
    if args.no_auto_start {
        return false;
    }
    !matches!(
        std::env::var("LOOM_NO_AUTO_START").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

/// Writes JSON to stdout or to the given file. When pretty is true, multi-line; else one line.
fn write_json_output(
    value: &serde_json::Value,
    file: Option<&std::path::Path>,
    pretty: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let s = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    match file {
        Some(path) => std::fs::write(path, format!("{}\n", s))?,
        None => {
            println!("{}", s);
            std::io::Write::flush(&mut std::io::stdout())?;
        }
    }
    Ok(())
}

/// Appends one JSON line to file or stdout (for NDJSON stream reply line).
fn write_json_line_append(
    value: &serde_json::Value,
    file: Option<&std::path::Path>,
    pretty: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let s = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    let line = format!("{}\n", s);
    match file {
        Some(path) => {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
            f.write_all(line.as_bytes())?;
        }
        None => {
            println!("{}", s.trim_end());
            std::io::Write::flush(&mut std::io::stdout())?;
        }
    }
    Ok(())
}

/// Builds StreamOut for --json --stream: writes each event as one JSON line to file or stdout.
fn make_stream_out(
    file: Option<&PathBuf>,
    pretty: bool,
) -> StreamOut {
    let file = file.cloned();
    Some(std::sync::Arc::new(std::sync::Mutex::new(move |value: serde_json::Value| {
        if value.get("type").and_then(|t| t.as_str()) == Some("node_enter") {
            if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
                eprintln!("Entering: {}", id);
            }
        }
        let s = if pretty {
            serde_json::to_string_pretty(&value).unwrap_or_default()
        } else {
            serde_json::to_string(&value).unwrap_or_default()
        };
        match &file {
            Some(path) => drop(std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| std::io::Write::write_all(&mut f, format!("{}\n", s).as_bytes()))),
            None => {
                println!("{}", s);
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        }
    })))
}

fn make_backend(args: &Args) -> Arc<dyn RunBackend> {
    if args.local {
        Arc::new(LocalBackend)
    } else {
        Arc::new(RemoteBackend::new(resolve_remote_url(args)))
    }
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    /// Run WebSocket server for remote mode (ws://127.0.0.1:8080)
    Serve(ServeArgs),
    /// Run ReAct graph (think → act → observe)
    React,
    /// Run DUP graph (understand → plan → act → observe)
    Dup,
    /// Run ToT graph (think_expand → think_evaluate → act → observe)
    Tot,
    /// Run GoT graph (plan_graph → execute_graph)
    Got(GotArgs),
    /// List or show tool definitions (same tools as used by react/dup/tot/got)
    Tool(ToolArgs),
}

#[derive(clap::Args, Debug, Clone)]
struct ToolArgs {
    #[command(subcommand)]
    sub: ToolCommand,
}

#[derive(Subcommand, Debug, Clone)]
enum ToolCommand {
    /// List all loaded tools (name and description)
    List,
    /// Show full definition of one tool (name, description, input_schema)
    Show(ShowToolArgs),
}

#[derive(clap::Args, Debug, Clone)]
struct ShowToolArgs {
    /// Tool name (e.g. read, web_fetcher)
    name: String,
    /// Output format: yaml (default) or json
    #[arg(long, value_name = "FORMAT", default_value = "yaml")]
    output: String,
}

/// Default max length for the *user message* sent to the agent (input truncation).
const DEFAULT_MAX_MESSAGE_LEN: usize = 200;

/// Default max length for the *reply* (assistant output) printed to stdout. 0 means no truncation.
const DEFAULT_MAX_REPLY_LEN: usize = 0;

/// Truncates `s` to at most `max` chars. When truncated, appends `...` (total length = max).
/// Uses character boundaries for safe UTF-8 handling.
fn truncate_message(s: &str, max: usize) -> String {
    const SUFFIX: &str = "...";
    let suffix_len = 3;
    if max <= suffix_len {
        return s.chars().take(max).collect();
    }
    let content_max = max - suffix_len;
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!(
        "{}{}",
        s.chars().take(content_max).collect::<String>(),
        SUFFIX
    )
}

/// Reads max message length from `HELVE_MAX_MESSAGE_LEN`. Returns default on missing/invalid.
fn max_message_len() -> usize {
    std::env::var("HELVE_MAX_MESSAGE_LEN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_MESSAGE_LEN)
}

/// Generates a session-unique thread ID for REPL mode when user does not provide one.
fn generate_repl_thread_id() -> String {
    format!(
        "thread-repl-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
}

/// Reads max reply length from `HELVE_MAX_REPLY_LEN`. 0 means no truncation. Returns default on missing/invalid.
fn max_reply_len() -> usize {
    std::env::var("HELVE_MAX_REPLY_LEN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_REPLY_LEN)
}

#[derive(clap::Args, Debug, Clone)]
struct ServeArgs {
    /// WebSocket listen address (default 127.0.0.1:8080)
    #[arg(long, value_name = "ADDR")]
    addr: Option<String>,
    /// Keep server running after first connection (default: exit after first connection)
    #[arg(long)]
    keep_alive: bool,
}

/// Arguments for the `got` subcommand.
#[derive(clap::Args, Debug, Clone)]
struct GotArgs {
    /// Enable AGoT adaptive mode (expand complex nodes).
    #[arg(long)]
    got_adaptive: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    config::load_and_apply("loom", None::<&std::path::Path>).ok();
    logging::init()?;

    let args = Args::parse();

    if let Some(Command::Serve(sa)) = &args.cmd {
        if let Err(e) = serve::run_serve(sa.addr.as_deref(), !sa.keep_alive).await {
            eprintln!("serve error: {}", e);
            std::process::exit(1);
        }
        return Ok(());
    }
    let got_adaptive = matches!(args.cmd.as_ref(), Some(Command::Got(a)) if a.got_adaptive);
    let backend = make_backend(&args);

    // For remote backend, ensure server is running (unless --no-auto-start)
    if !args.local && resolve_auto_start(&args) {
        let url = resolve_remote_url(&args);
        if let Err(e) = cli::ensure_server_or_spawn(&url).await {
            eprintln!("loom: {}", e);
            std::process::exit(1);
        }
    }

    // Tool subcommands do not require a message; handle them first.
    if let Some(Command::Tool(ta)) = &args.cmd {
        let opts = RunOptions {
            message: String::new(),
            working_folder: args.working_folder.clone(),
            thread_id: args.thread_id.clone(),
            verbose: args.verbose,
            got_adaptive,
            display_max_len: max_message_len(),
            output_json: args.json,
        };
        match &ta.sub {
            ToolCommand::List => {
                if let Err(e) = backend.list_tools(&opts).await {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
                return Ok(());
            }
            ToolCommand::Show(show_args) => {
                let format = if args.json || show_args.output.eq_ignore_ascii_case("json") {
                    ToolShowFormat::Json
                } else {
                    ToolShowFormat::Yaml
                };
                if let Err(e) = backend.show_tool(&opts, &show_args.name, format).await {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
                return Ok(());
            }
        }
    }

    let message = args
        .message
        .or_else(|| {
            if args.rest.is_empty() {
                None
            } else {
                Some(args.rest.join(" "))
            }
        });

    let interactive = args.interactive;
    if !interactive && message.is_none() {
        eprintln!("loom: provide a message via -m/--message or positional args");
        std::process::exit(1);
    }

    let mut opts = RunOptions {
        message: message.clone().unwrap_or_default(),
        working_folder: args.working_folder,
        thread_id: args.thread_id,
        verbose: args.verbose,
        got_adaptive,
        display_max_len: max_message_len(),
        output_json: args.json,
    };

    let cmd = args.cmd.unwrap_or(Command::React);
    let reply_len = max_reply_len();
    let stream_out: StreamOut = if args.json {
        make_stream_out(args.file.as_ref(), args.pretty)
    } else {
        None
    };

    if interactive {
        if opts.thread_id.is_none() {
            opts.thread_id = Some(generate_repl_thread_id());
        }
        if let Some(ref msg) = message {
            if !msg.trim().is_empty() {
                opts.message = msg.clone();
                match run_one_turn(&backend, &opts, &cmd, stream_out.clone()).await {
                    Ok(RunOutput::Reply(reply, reply_envelope)) => {
                        if args.json {
                            let mut out = serde_json::json!({ "reply": reply });
                            if let Some(ref env) = reply_envelope {
                                env.inject_into(&mut out);
                            }
                            if let Err(e) = write_json_line_append(&out, args.file.as_deref(), args.pretty) {
                                eprintln!("{}", e);
                                std::process::exit(1);
                            }
                        } else {
                            let out = if reply_len == 0 {
                                reply
                            } else {
                                truncate_message(&reply, reply_len)
                            };
                            println!("{}", out);
                        }
                    }
                    Ok(RunOutput::Json {
                        events,
                        reply,
                        reply_envelope,
                    }) => {
                        let mut reply_obj = serde_json::json!({ "reply": reply });
                        if let Some(ref env) = reply_envelope {
                            env.inject_into(&mut reply_obj);
                        }
                        let out = serde_json::json!({ "events": events, "reply": reply_obj });
                        if let Err(e) = write_json_output(&out, args.file.as_deref(), args.pretty) {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        run_repl_loop(
            &backend,
            &opts,
            &cmd,
            reply_len,
            args.file.clone(),
            args.pretty,
            stream_out,
        )
        .await?;
    } else {
        let output = run_one_turn(&backend, &opts, &cmd, stream_out).await?;
        match output {
            RunOutput::Reply(reply, reply_envelope) => {
                if args.json {
                    let mut out = serde_json::json!({ "reply": reply });
                    if let Some(ref env) = reply_envelope {
                        env.inject_into(&mut out);
                    }
                    write_json_line_append(&out, args.file.as_deref(), args.pretty)?;
                } else {
                    let out = if reply_len == 0 {
                        reply
                    } else {
                        truncate_message(&reply, reply_len)
                    };
                    println!("{}", out);
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
            }
            RunOutput::Json {
                events,
                reply,
                reply_envelope,
            } => {
                let mut reply_obj = serde_json::json!({ "reply": reply });
                if let Some(ref env) = reply_envelope {
                    env.inject_into(&mut reply_obj);
                }
                let out = serde_json::json!({ "events": events, "reply": reply_obj });
                write_json_output(&out, args.file.as_deref(), args.pretty)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{truncate_message, DEFAULT_MAX_MESSAGE_LEN, DEFAULT_MAX_REPLY_LEN};

    /// When message is shorter than max, returns unchanged.
    #[test]
    fn truncate_message_unchanged_when_short() {
        let s = "hello";
        assert_eq!(truncate_message(s, 200), "hello");
        assert_eq!(truncate_message(s, 10), "hello");
    }

    /// When message equals max, returns unchanged.
    #[test]
    fn truncate_message_unchanged_when_exact() {
        let s = "a".repeat(200);
        assert_eq!(truncate_message(&s, 200), s);
    }

    /// When message exceeds max, truncates to content_max + "..." (total = max).
    #[test]
    fn truncate_message_truncates_with_suffix() {
        let s = "a".repeat(250);
        let got = truncate_message(&s, 200);
        assert_eq!(got.len(), 200);
        assert!(got.ends_with("..."));
        assert_eq!(got.chars().count(), 200);
    }

    /// UTF-8 multi-byte chars are handled correctly (no panic, correct char count).
    #[test]
    fn truncate_message_utf8_safe() {
        let s = "Hello World ".repeat(20); // 240 chars
        let got = truncate_message(&s, 200);
        assert_eq!(got.chars().count(), 200);
        assert!(got.ends_with("..."));
    }

    /// Default max length constant is 200.
    #[test]
    fn default_max_message_len_is_200() {
        assert_eq!(DEFAULT_MAX_MESSAGE_LEN, 200);
    }

    /// Default reply length is 0 (no truncation; full assistant output).
    #[test]
    fn default_max_reply_len_is_zero() {
        assert_eq!(DEFAULT_MAX_REPLY_LEN, 0);
    }
}
