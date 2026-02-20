//! 自动拉起远端 server（`loom serve`）。
//!
//! 远端模式下（`--no-local`），CLI 需要连接 WebSocket 服务端执行 agent。
//! 这个模块在连接被拒绝（connection refused）时尝试自动启动服务端，
//! 提升“开箱即用”的体验：用户不必先手动运行 `loom serve`。
//!
//! 注意：这里的判定非常保守，只在明显的“连接被拒绝”情况下才会 spawn。
//! 其他错误（DNS、TLS、协议错误等）会原样返回，避免掩盖真实问题。

use std::process::Stdio;
use std::time::Duration;

const POLL_INTERVAL_MS: u64 = 200;
const MAX_WAIT_MS: u64 = 15000;

/// 在后台启动 `loom serve --keep-alive`。
///
/// - `--keep-alive`：让 server 在第一个连接结束后仍继续运行，便于后续多次 CLI 调用复用。
/// - stdout/stderr 被丢弃：CLI 的 stdout 需要保持干净（只输出 reply/JSON）。
pub fn spawn_serve() -> Result<std::process::Child, std::io::Error> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("serve")
        .arg("--keep-alive")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// 轮询 WebSocket URL，直到能连上或超时。
///
/// 这里使用“尝试连接成功”作为 readiness probe（不要求服务端返回特定业务响应）。
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

/// 确保远端 server 正在运行。
///
/// 流程：
/// 1. 先尝试连接一次。
/// 2. 若是 connection refused，则 spawn `loom serve`，等待就绪。
/// 3. 若最终能连上则返回 `Ok(())`。
///
/// 这里不会保留 WebSocket 连接：调用方（`RemoteBackend`）会自己重新建立连接并进行通信。
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
