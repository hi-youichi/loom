//! Test server binary for E2E testing
//! 
//! This binary starts the WebSocket server for E2E tests.
//! It binds to the specified port (default 8080) and runs continuously.

use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Get port from environment or use default
    let port: u16 = env::var("TEST_SERVER_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080);

    let addr = format!("127.0.0.1:{}", port);

    println!("🚀 Starting test server on ws://{}", addr);

    // Start the server (not in once mode, run continuously)
    serve::run_serve(Some(&addr), false).await?;

    Ok(())
}
