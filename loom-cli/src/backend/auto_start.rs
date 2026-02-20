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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn ensure_server_or_spawn_returns_error_for_invalid_url() {
        // Invalid URL should fail before spawn logic and return transport parsing error.
        let err = ensure_server_or_spawn("not-a-valid-url").await.unwrap_err();
        assert!(!err.to_lowercase().contains("server failed to become ready"));
    }

    #[tokio::test]
    async fn wait_for_server_and_ensure_server_or_spawn_succeed_when_server_up() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("ws://{}", addr);

        let server = tokio::spawn(async move {
            // one connection for wait_for_server, one for ensure_server_or_spawn
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let _ = tokio_tungstenite::accept_async(stream).await;
            }
        });

        assert!(wait_for_server(&url).await);
        assert!(ensure_server_or_spawn(&url).await.is_ok());
        server.await.unwrap();
    }
}
