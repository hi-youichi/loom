//! # loom-acp binary entrypoint
//!
//! IDEs (Zed, JetBrains, etc.) configure this executable as the ACP Agent command and communicate
//! over stdio. On startup we load [config](config) (same as loom), init tracing, then run
//! [`loom_acp::run_stdio_loop`] until stdin closes or an error occurs.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    config::load_and_apply("loom", None::<&std::path::Path>).ok();
    tracing_subscriber::fmt::init();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(loom_acp::run_stdio_loop())
}
