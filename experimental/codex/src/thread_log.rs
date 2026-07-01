use std::path::PathBuf;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::AsyncWriteExt;
use stream_event::codex::CodexEvent;

pub struct ThreadLog {
    file: File,
}

impl ThreadLog {
    pub async fn open(thread_id: &str) -> anyhow::Result<Self> {
        let dir = thread_log_dir();
        fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("{thread_id}.jsonl"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        Ok(Self { file })
    }

    /// Append one CodexEvent as a JSONL line.
    pub async fn append(&mut self, ev: &CodexEvent) -> anyhow::Result<()> {
        let line = serde_json::to_string(ev)?;
        self.file.write_all(line.as_bytes()).await?;
        self.file.write_all(b"\n").await?;
        self.file.flush().await?;
        Ok(())
    }
}

fn thread_log_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("loom-codex")
        .join("threads")
}
