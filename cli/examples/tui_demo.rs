//! TUI Demo - Demonstrates the TUI dashboard with simulated agent events.
//!
//! Run with: cargo run --example tui_demo

use cli::tui::{App, EventChannel, TuiEvent, TuiRunner};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Loom TUI Demo");
    println!("=============");
    println!();
    println!("This demo will launch a TUI dashboard showing simulated agent events.");
    println!("Press 'q' or Ctrl+C to quit the TUI.");
    println!();
    println!("Starting in 2 seconds...");
    sleep(Duration::from_secs(2)).await;

    // Create event channel
    let (event_channel, _receiver) = EventChannel::new();
    let sender = event_channel.sender();

    // Spawn a task to simulate agent events
    let event_sender = sender.clone();
    tokio::spawn(async move {
        // Agent 1: Dev agent
        event_sender
            .send(TuiEvent::AgentStarted {
                id: "agent-1".to_string(),
                name: "dev-agent".to_string(),
                task: "Implement user authentication feature".to_string(),
            })
            .unwrap();

        sleep(Duration::from_millis(500)).await;
        event_sender
            .send(TuiEvent::AgentProgress {
                id: "agent-1".to_string(),
                node: "think".to_string(),
                message: "Analyzing requirements...".to_string(),
            })
            .unwrap();

        sleep(Duration::from_secs(1)).await;
        event_sender
            .send(TuiEvent::AgentProgress {
                id: "agent-1".to_string(),
                node: "act".to_string(),
                message: "Writing authentication code...".to_string(),
            })
            .unwrap();

        // Agent 2: Code review agent
        sleep(Duration::from_millis(500)).await;
        event_sender
            .send(TuiEvent::AgentStarted {
                id: "agent-2".to_string(),
                name: "code-review".to_string(),
                task: "Review PR #123 for security issues".to_string(),
            })
            .unwrap();

        sleep(Duration::from_secs(1)).await;
        event_sender
            .send(TuiEvent::AgentProgress {
                id: "agent-2".to_string(),
                node: "analyze".to_string(),
                message: "Scanning code for vulnerabilities...".to_string(),
            })
            .unwrap();

        // Agent 3: Test runner
        sleep(Duration::from_millis(500)).await;
        event_sender
            .send(TuiEvent::AgentStarted {
                id: "agent-3".to_string(),
                name: "test-runner".to_string(),
                task: "Run integration tests".to_string(),
            })
            .unwrap();

        sleep(Duration::from_secs(1)).await;
        event_sender
            .send(TuiEvent::AgentProgress {
                id: "agent-3".to_string(),
                node: "execute".to_string(),
                message: "Running test suite...".to_string(),
            })
            .unwrap();

        // Complete test runner
        sleep(Duration::from_secs(2)).await;
        event_sender
            .send(TuiEvent::AgentCompleted {
                id: "agent-3".to_string(),
                result: "All tests passed successfully!".to_string(),
            })
            .unwrap();

        // Complete code review
        sleep(Duration::from_secs(1)).await;
        event_sender
            .send(TuiEvent::AgentCompleted {
                id: "agent-2".to_string(),
                result: "Code review complete. Found 2 minor issues.".to_string(),
            })
            .unwrap();

        // Dev agent encounters error
        sleep(Duration::from_secs(1)).await;
        event_sender
            .send(TuiEvent::AgentError {
                id: "agent-1".to_string(),
                error: "Failed to compile: missing dependency 'oauth2'".to_string(),
            })
            .unwrap();
    });

    // Run TUI
    let mut runner = TuiRunner::new();
    runner.run().await?;

    Ok(())
}
