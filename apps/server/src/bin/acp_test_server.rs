//! Deterministic ACP WebSocket fixture for Node.js black-box tests.

use std::sync::Arc;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = std::env::args().collect::<Vec<_>>();
    let port = args
        .iter()
        .skip_while(|arg| arg.as_str() != "--port")
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let db_path = args
        .iter()
        .skip_while(|arg| arg.as_str() != "--db-path")
        .nth(1)
        .map(std::path::PathBuf::from);
    let executor = Arc::new(anureo_acp::prompt_executor::DeterministicPromptExecutor);
    let runtime = match db_path {
        Some(path) => {
            anureo_acp::runtime::AcpRuntime::with_prompt_executor_and_db_path(executor, path)?
        }
        None => anureo_acp::runtime::AcpRuntime::with_prompt_executor(executor)?,
    };
    let hub = Arc::new(anureo_server::acp_hub::AcpHub::with_runtime(
        anureo_server::acp_hub::AcpHubConfig::default(),
        runtime,
    ));
    let state = anureo_server::state::new_server_state_with_acp_hub(hub);
    let app = anureo_server::routes::build_router(state);
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let address = listener.local_addr()?;
    println!("ACP_TEST_SERVER_URL=ws://{address}/acp");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
