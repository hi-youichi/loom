//! Interactive REPL loop: read stdin, run agent, print output, repeat until EOF or quit.
//!
//! Used when `-i/--interactive` is passed. Ensures a stable `thread_id` for multi-turn history.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};

use cli::{RunBackend, RunCmd, RunError, RunOptions};

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
    stream_out: cli::StreamOut,
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
            Ok(cli::RunOutput::Json {
                events,
                reply,
                reply_envelope,
            }) => {
                let mut reply_obj = serde_json::json!({ "reply": reply });
                if let Some(ref env) = reply_envelope {
                    env.inject_into(&mut reply_obj);
                }
                let out = serde_json::json!({ "events": events, "reply": reply_obj });
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
            Ok(cli::RunOutput::Reply(reply, reply_envelope)) => {
                if json_stream {
                    let mut out = serde_json::json!({ "reply": reply });
                    if let Some(ref env) = reply_envelope {
                        env.inject_into(&mut out);
                    }
                    let s = if json_pretty {
                        serde_json::to_string_pretty(&out).unwrap_or_default()
                    } else {
                        serde_json::to_string(&out).unwrap_or_default()
                    };
                    match &json_file {
                        Some(p) => {
                            use std::io::Write;
                            let mut f = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(p)?;
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
    stream_out: cli::StreamOut,
) -> Result<cli::RunOutput, RunError> {
    let run_cmd = cmd_to_runcmd(cmd);
    backend.run(opts, &run_cmd, stream_out).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    struct DummyBackend {
        seen: Arc<Mutex<Vec<RunCmd>>>,
    }

    #[async_trait]
    impl RunBackend for DummyBackend {
        async fn run(
            &self,
            _opts: &RunOptions,
            cmd: &RunCmd,
            _stream_out: cli::StreamOut,
        ) -> Result<cli::RunOutput, RunError> {
            self.seen.lock().unwrap().push(cmd.clone());
            Ok(cli::RunOutput::Reply("ok".to_string(), None))
        }

        async fn list_tools(&self, _opts: &RunOptions) -> Result<(), RunError> {
            Ok(())
        }

        async fn show_tool(
            &self,
            _opts: &RunOptions,
            _name: &str,
            _format: cli::ToolShowFormat,
        ) -> Result<(), RunError> {
            Ok(())
        }
    }

    #[test]
    fn is_quit_command_matches_expected_tokens() {
        assert!(is_quit_command("quit"));
        assert!(is_quit_command(" EXIT "));
        assert!(is_quit_command("/quit"));
        assert!(!is_quit_command("continue"));
    }

    #[test]
    fn truncate_reply_respects_zero_and_limit() {
        assert_eq!(truncate_reply("hello world", 0), "hello world");
        let truncated = truncate_reply("abcdefghijk", 8);
        assert_eq!(truncated.chars().count(), 8);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn cmd_to_runcmd_maps_basic_variants() {
        assert!(matches!(cmd_to_runcmd(&Command::React), RunCmd::React));
        assert!(matches!(cmd_to_runcmd(&Command::Dup), RunCmd::Dup));
        assert!(matches!(cmd_to_runcmd(&Command::Tot), RunCmd::Tot));
    }

    #[tokio::test]
    async fn run_one_turn_delegates_to_backend_with_mapped_cmd() {
        let seen = Arc::new(Mutex::new(Vec::<RunCmd>::new()));
        let backend: Arc<dyn RunBackend> = Arc::new(DummyBackend {
            seen: Arc::clone(&seen),
        });
        let opts = RunOptions {
            message: "hello".to_string(),
            working_folder: None,
            thread_id: None,
            verbose: false,
            got_adaptive: false,
            display_max_len: 100,
            output_json: false,
        };

        let out = run_one_turn(&backend, &opts, &Command::Dup, None)
            .await
            .unwrap();
        assert!(matches!(out, cli::RunOutput::Reply(reply, _) if reply == "ok"));
        assert!(matches!(seen.lock().unwrap().first(), Some(RunCmd::Dup)));
    }
}
