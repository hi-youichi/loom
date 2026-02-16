//! Interactive REPL loop: read stdin, run agent, print output, repeat until EOF or quit.
//!
//! Used when `-i/--interactive` is passed. Ensures a stable `thread_id` for multi-turn history.

use std::io::Write;

use tokio::io::{AsyncBufReadExt, BufReader};

use graphweave_cli::{run_dup, run_got, run_react, run_tot, RunError, RunOptions};

use crate::Command;

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
    base_opts: &RunOptions,
    cmd: &Command,
    max_reply_len: usize,
) -> Result<(), Box<dyn std::error::Error>> {
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

        match run_one_turn(&opts, cmd).await {
            Ok(reply) => println!("{}", truncate_reply(&reply, max_reply_len)),
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
pub async fn run_one_turn(opts: &RunOptions, cmd: &Command) -> Result<String, RunError> {
    match cmd {
        Command::React => run_react(opts).await,
        Command::Dup => run_dup(opts).await,
        Command::Tot => run_tot(opts).await,
        Command::Got(_) => run_got(opts).await,
        Command::Tool(_) => unreachable!("tool handled in main"),
    }
}
