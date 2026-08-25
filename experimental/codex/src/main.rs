mod agent;
mod approval;
mod event_bridge;
mod stdio_loop;
mod thread_log;

use clap::Parser;

#[derive(Parser, Clone, Debug)]
#[command(name = "anureo-codex", about = "anureo codex app-server compatible daemon")]
pub struct CodexServerArgs {
    #[arg(long, default_value = "claude-sonnet-4-6")]
    pub model: String,
    #[arg(long)]
    pub cwd: Option<std::path::PathBuf>,
    #[arg(long)]
    pub system_prompt: Option<String>,
    #[arg(long, default_value = "on-request")]
    pub approval_policy: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = CodexServerArgs::parse();
    stdio_loop::run_codex_stdio_loop(args).await
}
