//! Interactive REPL loop: read stdin, run agent, print output, repeat until EOF or quit.
//!
//! Used when `-i/--interactive` is passed. Ensures a stable `session_id` for multi-turn history.

use std::io::Write;

use tokio::io::{AsyncBufReadExt, BufReader};

use cli::{run_cli_turn, RunCmd, RunError, RunOptions, RunOutput, StreamOut};

use crate::output::{emit_run_output, OutputConfig};
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
        Command::Session(_) => unreachable!("session handled in main"),
        Command::Models(_) => unreachable!("models handled in main"),
    }
}

/// Runs the REPL loop: prompt, read line, run agent, print, repeat.
///
/// Exits on EOF (Ctrl+D), empty line, or `quit`/`exit`/`/quit`.
/// On run error, prints to stderr and continues.
pub async fn run_repl_loop(
    base_opts: &RunOptions,
    cmd: &Command,
    max_reply_len: usize,
    output: OutputConfig,
    stream_out: StreamOut,
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

        match run_one_turn(&opts, cmd, stream_out.clone()).await {
            Ok(output_value) => emit_run_output(
                output_value,
                &output,
                opts.thread_id.as_deref(),
                max_reply_len,
                base_opts.output_timestamp,
            )?,
            Err(e) => eprintln!("error: {}", e),
        }
    }

    Ok(())
}

fn is_quit_command(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    matches!(lower.as_str(), "quit" | "exit" | "/quit")
}

/// Runs one turn of the agent (react, dup, tot, or got).
pub async fn run_one_turn(
    opts: &RunOptions,
    cmd: &Command,
    stream_out: StreamOut,
) -> Result<RunOutput, RunError> {
    let run_cmd = cmd_to_runcmd(cmd);
    run_cli_turn(opts, &run_cmd, stream_out).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_quit_command_matches_expected_tokens() {
        assert!(is_quit_command("quit"));
        assert!(is_quit_command(" EXIT "));
        assert!(is_quit_command("/quit"));
        assert!(!is_quit_command("continue"));
    }

    #[test]
    fn cmd_to_runcmd_maps_basic_variants() {
        assert!(matches!(cmd_to_runcmd(&Command::React), RunCmd::React));
        assert!(matches!(cmd_to_runcmd(&Command::Dup), RunCmd::Dup));
        assert!(matches!(cmd_to_runcmd(&Command::Tot), RunCmd::Tot));
    }
}
