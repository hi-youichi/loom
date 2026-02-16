//! Logging initialization: logs go only to file (or are dropped), never to console.
//!
//! Reads `RUST_LOG` (level) and `LOG_FILE` (path) from env (e.g. via .env).
//! When `LOG_FILE` is set, logs are appended to that file; otherwise logs are dropped
//! so the CLI stdout stays clean for the reply only.

use std::io::Write;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

/// Initializes tracing so that logs are never printed to the console.
///
/// - **RUST_LOG**: Log level filter, e.g. `info`, `debug`, `graphweave=debug`. Default: `info`.
/// - **LOG_FILE**: When set, logs are appended to this file (plain text, no ANSI).
///   When unset, logs are dropped (sink) so only the CLI reply is shown on stdout.
pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,hyper_util=off"));

    if let Ok(path) = std::env::var("LOG_FILE") {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let writer = std::sync::Mutex::new(StripAnsiWriter::new(file));
        let file_layer = tracing_subscriber::fmt::layer()
            .event_format(crate::log_format::TextWithSpanIds::new())
            .with_writer(writer)
            .with_ansi(false)
            .with_filter(filter);
        tracing_subscriber::registry().with(file_layer).init();
        tracing::info!(path = %path, "graphweave-cli logging to file");
    } else {
        let sink_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::sink)
            .with_filter(filter);
        tracing_subscriber::registry().with(sink_layer).init();
    }
    Ok(())
}

/// Strips ANSI escape sequences so file logs are plain text.
struct StripAnsiWriter<W> {
    inner: W,
    state: Vec<u8>,
}

impl<W: Write> StripAnsiWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            state: Vec::with_capacity(16),
        }
    }
}

impl<W: Write> Write for StripAnsiWriter<W> {
    fn write(&mut self, mut buf: &[u8]) -> std::io::Result<usize> {
        let len = buf.len();
        while !buf.is_empty() {
            if self.state.is_empty() {
                if let Some(i) = buf.iter().position(|&b| b == 0x1b) {
                    self.inner.write_all(&buf[..i])?;
                    buf = &buf[i..];
                    self.state.push(buf[0]);
                    buf = &buf[1..];
                } else {
                    self.inner.write_all(buf)?;
                    break;
                }
            } else if self.state.len() == 1 {
                self.state.push(buf[0]);
                buf = &buf[1..];
                if self.state[1] != b'[' {
                    self.inner.write_all(&self.state)?;
                    self.state.clear();
                }
            } else {
                let b = buf[0];
                buf = &buf[1..];
                let is_csi_final = b >= 0x40 && b <= 0x7e;
                let is_csi_param = b == b'[' || b == b'?' || b == b';' || (b >= b'0' && b <= b'9');
                if is_csi_final {
                    self.state.clear();
                } else if is_csi_param || b == b':' {
                    self.state.push(b);
                    if self.state.len() > 64 {
                        self.inner.write_all(&self.state)?;
                        self.state.clear();
                    }
                } else {
                    self.inner.write_all(&self.state)?;
                    self.state.clear();
                    self.state.push(b);
                }
            }
        }
        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.state.is_empty() {
            self.inner.write_all(&self.state)?;
            self.state.clear();
        }
        self.inner.flush()
    }
}
