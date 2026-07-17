//! Interactive REPL loop: read stdin, run agent, print output, repeat until EOF or quit.
//!
//! Used when `-i/--interactive` is passed. Ensures a stable `session_id` for multi-turn history.

use std::io::Write;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Notify;

use agent::commands::{self as loom_command};
use cli::{run_cli_turn, RunCmd, RunError, RunOptions, RunOutput};
use loom_llm::message::UserContent;

use crate::output::{emit_run_output, EventSink, OutputConfig};
use crate::Command;

fn cmd_to_runcmd(cmd: &Command) -> RunCmd {
    match cmd {
        Command::React => RunCmd::React,
        Command::Dup => RunCmd::Dup,
        Command::Tot => RunCmd::Tot,
        Command::Got(a) => RunCmd::Got {
            got_adaptive: a.got_adaptive,
        },
        Command::Tool(_) => unreachable!("tool handled in main"),
        Command::Session(_) => unreachable!("session handled in main"),
        Command::Models(_) => unreachable!("models handled in main"),
        Command::Mcp(_) => unreachable!("mcp handled in main"),
        Command::Agent(_) => unreachable!("agent handled in main"),
        Command::Goal(_) => unreachable!("goal handled in main"),
        Command::Skills(_) => unreachable!("skills handled in main"),
        Command::SkillUsage(_) => unreachable!("skill-usage handled in main"),
        Command::Evolve => unreachable!("evolve handled in main"),

        Command::Curator(_) => unreachable!("curator handled in main"),
        Command::Memory(_) => unreachable!("memory handled in main"),
        Command::ReviewSkill(_) => unreachable!("review-skill handled in main"),
        Command::Review(_) => unreachable!("review handled in main"),
        Command::Task(_) => unreachable!("task handled in main"),

        Command::Acp(_) => unreachable!("acp handled in main"),
    }
}

pub async fn run_repl_loop(
    base_opts: &RunOptions,
    cmd: &Command,
    max_reply_len: usize,
    output: OutputConfig,
    stream_out: EventSink,
    force_quit: Arc<Notify>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(tokio::io::stdin()).lines();

    loop {
        print!("> ");
        std::io::stdout().flush()?;

        // Use select! so Ctrl+C can interrupt the stdin wait.
        // Without this, the REPL blocks on next_line() forever even after
        // the ctrlc handler calls CancellationToken::cancel().
        let line = tokio::select! {
            result = reader.next_line() => result?,
            _ = force_quit.notified() => {
                eprintln!();
                break;
            }
        };

        let line = match line {
            None => break,
            Some(s) if s.trim().is_empty() => continue,
            Some(s) if is_quit_command(&s) => break,
            Some(s) => s,
        };

        if let Some(parsed) = loom_command::parse(&line) {
            match parsed {
                loom_command::Command::Models { .. } | loom_command::Command::ModelsUse { .. } => {
                    println!("/models is not yet supported in CLI mode.");
                }
                _ => {
                    let reply = handle_repl_command(parsed);
                    println!("{}", reply);
                }
            }
            continue;
        }

        let mut opts = base_opts.clone();
        opts.message = UserContent::Text(line);

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

fn handle_repl_command(cmd: loom_command::Command) -> String {
    match cmd {
        loom_command::Command::ResetContext => "Context will be cleared on next run.".into(),
        loom_command::Command::Compact { .. } => {
            "/compact requires an active session with LLM access.".into()
        }
        loom_command::Command::Summarize => {
            "/summarize requires an active session with LLM access.".into()
        }
        loom_command::Command::Models { .. } | loom_command::Command::ModelsUse { .. } => {
            unreachable!("handled above")
        }
        loom_command::Command::Goal { .. } => {
            "/goal requires an active session with LLM access.".into()
        }
loom_command::Command::ReviewSkill { .. } => {
            "/review-skill requires an active session with LLM access.".into()
        }
        // Priority #18 (Hermes parity, `cli.py`): placeholder
        // replies for the high-value slash commands added in
        // `agent/agent-core/src/commands/parser.rs`. Real executors
        // would call into SessionManager / model catalog; for now
        // each prints a one-line stub so the parser round-trip
        // works and the user sees feedback immediately.
        loom_command::Command::Help { command } => match command {
            Some(c) => format!("/help {}: see the docs for `{}`.", c, c),
            None => "/help: /reset /compact /summarize /models /model /goal /review-skill /tools /resume /undo /retry /history /exit".into(),
        },
        loom_command::Command::Tools => {
            "/tools: enumerated tool list (stub — see agent/agent-core/src/tools for the registry).".into()
        }
        loom_command::Command::Model { model_id } => match model_id {
            Some(id) => format!("/model {}: switching model (stub — handler in subcommands).", id),
            None => "/model <id>: switch the active model; run /models to list.".into(),
        },
        loom_command::Command::Resume { selector } => match selector {
            Some(s) => format!("/resume {}: resuming session (stub).", s),
            None => "/resume <title|id>: pick a previous session.".into(),
        },
        loom_command::Command::Undo => {
            "/undo: rolling back the most recent assistant turn (stub — see SessionManager::restore_rewound).".into()
        }
        loom_command::Command::Retry => {
            "/retry: re-running the most recent user prompt (stub).".into()
        }
        loom_command::Command::History { count } => {
            format!("/history: showing last {} user prompts (stub).", count.unwrap_or(10))
        }
        loom_command::Command::Exit => "/exit: leaving the REPL (stub).".into(),
    }
}

fn is_quit_command(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    matches!(lower.as_str(), "quit" | "exit" | "/quit")
}

pub async fn run_one_turn(
    opts: &RunOptions,
    cmd: &Command,
    stream_out: EventSink,
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
