//! Shared CLI output helpers for stdout/file and JSON/text modes.

use cli::{Envelope, RunOutput, RunStopReason, StreamOut};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub(crate) struct OutputConfig {
    pub json: bool,
    pub pretty: bool,
    pub file: Option<PathBuf>,
}

pub(crate) fn write_json_output(
    value: &Value,
    file: Option<&Path>,
    pretty: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let serialized = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };

    match file {
        Some(path) => std::fs::write(path, format!("{serialized}\n"))?,
        None => {
            println!("{serialized}");
            std::io::stdout().flush()?;
        }
    }

    Ok(())
}

pub(crate) fn append_json_line(
    value: &Value,
    file: Option<&Path>,
    pretty: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let serialized = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    let line = format!("{serialized}\n");

    match file {
        Some(path) => {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            file.write_all(line.as_bytes())?;
        }
        None => {
            print!("{line}");
            std::io::stdout().flush()?;
        }
    }

    Ok(())
}

/// Builds the event sink used by `--json` mode.
pub(crate) fn make_stream_out(config: &OutputConfig) -> StreamOut {
    if !config.json {
        return None;
    }

    let file = config.file.clone();
    let pretty = config.pretty;
    Some(std::sync::Arc::new(std::sync::Mutex::new(
        move |value: Value| {
            if value.get("type").and_then(Value::as_str) == Some("node_enter") {
                if let Some(id) = value.get("id").and_then(Value::as_str) {
                    eprintln!("Entering: {}", id);
                }
            }

            let serialized = if pretty {
                serde_json::to_string_pretty(&value).unwrap_or_default()
            } else {
                serde_json::to_string(&value).unwrap_or_default()
            };

            match &file {
                Some(path) => drop(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .and_then(|mut file| file.write_all(format!("{serialized}\n").as_bytes())),
                ),
                None => {
                    println!("{serialized}");
                    let _ = std::io::stdout().flush();
                }
            }
        },
    )))
}

pub(crate) fn emit_run_output(
    output: RunOutput,
    config: &OutputConfig,
    session_id: Option<&str>,
    max_reply_len: usize,
    timestamp: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match output {
        RunOutput::Reply {
            reply,
            reasoning_content,
            reply_envelope,
            stop_reason,
        } => {
            if config.json {
                let mut out = reply_value(reply, reasoning_content, reply_envelope);
                out["stop_reason"] = serde_json::json!(stop_reason_str(stop_reason));
                if let Some(session_id) = session_id {
                    out["session_id"] = serde_json::json!(session_id);
                }
                append_json_line(&out, config.file.as_deref(), config.pretty)?;
            } else {
                emit_text_reply(&reply, max_reply_len, timestamp)?;
            }
        }
        RunOutput::Json {
            events,
            reply,
            reasoning_content,
            reply_envelope,
            stop_reason,
        } => {
            let mut out = serde_json::json!({
                "events": events,
                "reply": reply_value(reply, reasoning_content, reply_envelope),
                "stop_reason": stop_reason_str(stop_reason),
            });
            if let Some(session_id) = session_id {
                out["session_id"] = serde_json::json!(session_id);
            }
            write_json_output(&out, config.file.as_deref(), config.pretty)?;
        }
    }

    Ok(())
}

fn emit_text_reply(
    reply: &str,
    max_reply_len: usize,
    timestamp: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if timestamp {
        cli::run::print_reply_timestamp();
    }

    let text = if max_reply_len == 0 {
        reply.to_string()
    } else {
        crate::display_limits::truncate_message(reply, max_reply_len)
    };
    println!("{text}");
    std::io::stdout().flush()?;
    Ok(())
}

fn reply_value(
    reply: String,
    reasoning_content: Option<String>,
    reply_envelope: Option<Envelope>,
) -> Value {
    let mut out = serde_json::json!({ "reply": reply });
    if let Some(reasoning_content) = reasoning_content {
        out["reasoning_content"] = serde_json::json!(reasoning_content);
    }
    if let Some(ref envelope) = reply_envelope {
        envelope.inject_into(&mut out);
    }
    out
}

fn stop_reason_str(stop_reason: RunStopReason) -> &'static str {
    match stop_reason {
        RunStopReason::EndTurn => "end_turn",
        RunStopReason::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_json_output_and_append_write_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out.json");
        let value = serde_json::json!({"a":1});

        write_json_output(&value, Some(file.as_path()), false).unwrap();
        let first = std::fs::read_to_string(&file).unwrap();
        assert_eq!(first.trim(), r#"{"a":1}"#);

        let second = serde_json::json!({"b":2});
        append_json_line(&second, Some(file.as_path()), false).unwrap();
        let all = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = all.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1], r#"{"b":2}"#);
    }

    #[test]
    fn make_stream_out_writes_ndjson_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("stream.ndjson");
        let config = OutputConfig {
            json: true,
            pretty: false,
            file: Some(file.clone()),
        };
        let out = make_stream_out(&config).unwrap();
        if let Ok(mut f) = out.lock() {
            f(serde_json::json!({"type":"node_enter","id":"think"}));
            f(serde_json::json!({"type":"usage","total_tokens":3}));
        }
        let content = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(r#""type":"node_enter""#));
        assert!(lines[1].contains(r#""type":"usage""#));
    }
}
