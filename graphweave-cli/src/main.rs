//! GraphWeave CLI binary: run ReAct or DUP agent from the command line.
//!
//! Subcommands: `react` (default ReAct), `dup` (DUP), `tot` (ToT), `got` (GoT), `tool` (list/show tools).

mod log_format;
mod logging;
mod repl;

use clap::{Parser, Subcommand};
use graphweave_cli::{list_tools, show_tool, RunOptions, ToolShowFormat};
use repl::{run_one_turn, run_repl_loop};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "graphweave")]
#[command(about = "GraphWeave — run ReAct or DUP agent from CLI")]
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
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
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
    /// Tool name (e.g. read_file, web_fetcher)
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

/// Arguments for the `got` subcommand.
#[derive(clap::Args, Debug, Clone)]
struct GotArgs {
    /// Enable AGoT adaptive mode (expand complex nodes).
    #[arg(long)]
    got_adaptive: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    logging::init()?;

    let args = Args::parse();
    let got_adaptive = matches!(args.cmd.as_ref(), Some(Command::Got(a)) if a.got_adaptive);

    // Tool subcommands do not require a message; handle them first.
    if let Some(Command::Tool(ta)) = &args.cmd {
        let opts = RunOptions {
            message: String::new(),
            working_folder: args.working_folder,
            thread_id: args.thread_id,
            verbose: args.verbose,
            got_adaptive,
            display_max_len: max_message_len(),
        };
        match &ta.sub {
            ToolCommand::List => {
                list_tools(&opts).await?;
                return Ok(());
            }
            ToolCommand::Show(show_args) => {
                let format = if show_args.output.eq_ignore_ascii_case("json") {
                    ToolShowFormat::Json
                } else {
                    ToolShowFormat::Yaml
                };
                if let Err(e) = show_tool(&opts, &show_args.name, format).await {
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
        eprintln!("graphweave: provide a message via -m/--message or positional args");
        std::process::exit(1);
    }

    let mut opts = RunOptions {
        message: message.clone().unwrap_or_default(),
        working_folder: args.working_folder,
        thread_id: args.thread_id,
        verbose: args.verbose,
        got_adaptive,
        display_max_len: max_message_len(),
    };

    let cmd = args.cmd.unwrap_or(Command::React);
    let reply_len = max_reply_len();

    if interactive {
        if opts.thread_id.is_none() {
            opts.thread_id = Some(generate_repl_thread_id());
        }
        if let Some(ref msg) = message {
            if !msg.trim().is_empty() {
                opts.message = msg.clone();
                match run_one_turn(&opts, &cmd).await {
                    Ok(reply) => {
                        let out = if reply_len == 0 {
                            reply
                        } else {
                            truncate_message(&reply, reply_len)
                        };
                        println!("{}", out);
                    }
                    Err(e) => {
                        eprintln!("error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        run_repl_loop(&opts, &cmd, reply_len).await?;
    } else {
        let reply = run_one_turn(&opts, &cmd).await?;
        let out = if reply_len == 0 {
            reply
        } else {
            truncate_message(&reply, reply_len)
        };
        println!("{}", out);
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
