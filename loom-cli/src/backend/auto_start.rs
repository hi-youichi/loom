//! Auto-start server when remote connection fails.

use std::process::Stdio;
use std::time::Duration;

const POLL_INTERVAL_MS: u64 = 200;
const MAX_WAIT_MS: u64 = 15000;

/// Spawns `loom serve --keep-alive` in the background so the server stays up for this
/// and future runs; returns once the process is started.
pub fn spawn_serve() -> Result<std::process::Child, std::io::Error> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("serve")
        .arg("--keep-alive")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Polls the WebSocket URL until it accepts connections or timeout.
pub async fn wait_for_server(url: &str) -> bool {
    let start = std::time::Instant::now();
    let max_wait = Duration::from_millis(MAX_WAIT_MS);
    let interval = Duration::from_millis(POLL_INTERVAL_MS);

    while start.elapsed() < max_wait {
        if tokio_tungstenite::connect_async(url).await.is_ok() {
            return true;
        }
        tokio::time::sleep(interval).await;
    }
    false
}

/// Tries to connect to url. On connection refused, spawns serve and retries.
/// Returns Ok(()) if connect would succeed (we don't keep the connection).
pub async fn ensure_server_or_spawn(url: &str) -> Result<(), String> {
    match tokio_tungstenite::connect_async(url).await {
        Ok(_) => return Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if !msg.contains("refused") && !msg.contains("Connection refused") {
                return Err(msg);
            }
        }
    }

    eprintln!("loom: remote not running, starting server...");
    spawn_serve().map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    if wait_for_server(url).await {
        Ok(())
    } else {
        Err("server failed to become ready".to_string())
    }
}
