//! Shared helpers for e2e tests. Received responses are logged with `[e2e] received: ...`.
//! Run tests with `--nocapture` to see them.

use futures_util::{SinkExt, StreamExt};

/// Loads .env from the current directory (or project root when run via `cargo test`).
/// Call at the start of each e2e test so the server and config see OPENAI_API_KEY etc.
pub fn load_dotenv() {
    let _ = dotenv::dotenv();
}

use loom::{ClientRequest, ServerResponse};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

/// Returns the parsed response and the raw received JSON so tests can assert on wire content.
pub async fn send_and_recv<W, R>(
    write: &mut W,
    read: &mut R,
    req: &ClientRequest,
) -> Result<(ServerResponse, String), Box<dyn std::error::Error + Send + Sync>>
where
    W: SinkExt<Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let json = serde_json::to_string(req)?;
    write.send(Message::Text(json)).await?;
    let read_timeout = Duration::from_secs(10);
    let opt = timeout(read_timeout, read.next())
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout waiting for response"))?;
    let msg = opt.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "no message"))??;
    let text = msg.to_text().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let received = text.to_string();
    eprintln!("[e2e] received: {}", received);
    let resp: ServerResponse = serde_json::from_str(text)?;
    Ok((resp, received))
}

/// Sends a Run request and reads until RunEnd or Error (matching id).
/// Returns the final response and the raw JSON of that message so tests can assert on wire content.
pub async fn send_run_and_recv_end<W, R>(
    write: &mut W,
    read: &mut R,
    req: &ClientRequest,
    read_timeout: Duration,
) -> Result<(ServerResponse, String), Box<dyn std::error::Error + Send + Sync>>
where
    W: SinkExt<Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let id = match req {
        ClientRequest::Run(r) => r.id.clone(),
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "expected Run request",
            )
            .into())
        }
    };
    let json = serde_json::to_string(req)?;
    write.send(Message::Text(json)).await?;
    loop {
        let opt = timeout(read_timeout, read.next())
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout waiting for run_end"))?;
        let msg = opt.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "no message"))??;
        if !msg.is_text() {
            continue;
        }
        let text = msg.to_text().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let received = text.to_string();
        eprintln!("[e2e] received: {}", received);
        let resp: ServerResponse = serde_json::from_str(text)?;
        match &resp {
            ServerResponse::RunEnd(r) if r.id == id => return Ok((resp, received)),
            ServerResponse::Error(e) if e.id.as_deref() == Some(id.as_str()) => return Ok((resp, received)),
            ServerResponse::RunStreamEvent(ev) if ev.id == id => continue,
            _ => continue,
        }
    }
}
