//! Interactive REPL loop: read stdin, run agent, print output, repeat until EOF or quit.
//!
//! Used when `-i/--interactive` is passed. Ensures a stable `thread_id` for multi-turn history.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};

use loom_cli::{RunBackend, RunCmd, RunError, RunOptions};

use crate::Command;

fn cmd_to_runcmd(cmd: &Command) -> RunCmd {
    match cmd {
        Command::Serve(_) => unreachable!("serve handled in main"),
        Command::React => RunCmd::React,
        Command::Dup => RunCmd::Dup,
        Command::Tot => RunCmd::Tot,
        Command::Got(a) => RunCmd::Got {
            got_adaptive: a.got_adaptive,
        },
        Command::Tool(_) => unreachable!("tool handled in main"),
    }
}

/// Truncates reply for display. 0 means no truncation.
fn truncate_reply(reply: &str, max_len: usize) -> String {
    if max_len == 0 {
        return reply.to_string();
    }
    crate::truncate_message(reply, max_len)
}

/// Runs the REPL loop: prompt, read line, run agent, print, repeat.
///
/// Exits on EOF (Ctrl+D), empty line, or `quit`/`exit`/`/quit`.
/// On run error, prints to stderr and continues.
pub async fn run_repl_loop(
    backend: &Arc<dyn RunBackend>,
    base_opts: &RunOptions,
    cmd: &Command,
    max_reply_len: usize,
    json_file: Option<PathBuf>,
    json_pretty: bool,
    stream_out: loom_cli::StreamOut,
) -> Result<(), Box<dyn std::error::Error>> {
    let json_stream = stream_out.is_some();
    let mut reader = BufReader::new(tokio::io::stdin()).lines();

    loop {
        print!("> ");
        std::io::stdout().flush()?;

        let line = reader.next_line().await?;

        let line = match line {
            None => break,
            Some(s) if s.trim().is_empty() => continue,
            Some(s) if is_quit_command(&s) => break,
            Some(s) => s,
        };

        let mut opts = base_opts.clone();
        opts.message = line;

        match run_one_turn(backend, &opts, cmd, stream_out.clone()).await {
            Ok(loom_cli::RunOutput::Json { events, reply }) => {
                let out = serde_json::json!({ "events": events, "reply": reply });
                let s = if json_pretty {
                    serde_json::to_string_pretty(&out).unwrap_or_default()
                } else {
                    serde_json::to_string(&out).unwrap_or_default()
                };
                match &json_file {
                    Some(p) => std::fs::write(p, format!("{}\n", s))?,
                    None => println!("{}", s),
                }
            }
            Ok(loom_cli::RunOutput::Reply(reply)) => {
                if json_stream {
                    let out = serde_json::json!({ "reply": reply });
                    let s = if json_pretty {
                        serde_json::to_string_pretty(&out).unwrap_or_default()
                    } else {
                        serde_json::to_string(&out).unwrap_or_default()
                    };
                    match &json_file {
                        Some(p) => {
                            use std::io::Write;
                            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
                            f.write_all(format!("{}\n", s).as_bytes())?;
                        }
                        None => println!("{}", s),
                    }
                } else {
                    println!("{}", truncate_reply(&reply, max_reply_len));
                }
            }
            Err(e) => eprintln!("error: {}", e),
        }
    }

    println!("Bye.");
    Ok(())
}

fn is_quit_command(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    matches!(lower.as_str(), "quit" | "exit" | "/quit")
}

/// Runs one turn of the agent (react, dup, tot, or got).
pub async fn run_one_turn(
    backend: &Arc<dyn RunBackend>,
    opts: &RunOptions,
    cmd: &Command,
    stream_out: loom_cli::StreamOut,
) -> Result<loom_cli::RunOutput, RunError> {
    let run_cmd = cmd_to_runcmd(cmd);
    backend.run(opts, &run_cmd, stream_out).await
}
