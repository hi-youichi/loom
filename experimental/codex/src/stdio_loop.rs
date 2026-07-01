use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use crate::agent::{CodexAgent, OutputEvent};
use crate::CodexServerArgs;

#[derive(Serialize, Clone, Debug)]
pub struct CodexNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    pub params: serde_json::Value,
}

pub async fn run_codex_stdio_loop(args: CodexServerArgs) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let (output_tx, mut output_rx) = mpsc::channel::<OutputEvent>(256);
    let agent = Arc::new(CodexAgent::new(args, output_tx)?);

    // drain task: output_rx → stdout
    let drain = tokio::spawn(async move {
        while let Some(event) = output_rx.recv().await {
            let line = match event {
                OutputEvent::Notification(n) => serde_json::to_string(&n),
                OutputEvent::Response { id, result } => {
                    serde_json::to_string(&json!({"jsonrpc":"2.0","id":id,"result":result}))
                }
                OutputEvent::ErrorResponse { id, code, message } => {
                    serde_json::to_string(&json!({
                        "jsonrpc":"2.0","id":id,
                        "error":{"code":code,"message":message}
                    }))
                }
            };
            if let Ok(line) = line {
                println!("{line}");
            }
        }
    });

    // stdin reader loop
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() { continue; }
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(msg) => {
                let agent = agent.clone();
                tokio::spawn(async move { agent.handle(msg).await });
            }
            Err(e) => {
                tracing::warn!("Failed to parse stdin line: {e}");
            }
        }
    }

    drop(agent);
    let _ = drain.await;
    Ok(())
}
