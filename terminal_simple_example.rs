//! A simple example demonstrating how to send terminal requests and receive responses
//! using the Agent Client Protocol (ACP).

use agent_client_protocol::{
    ClientSideConnection, 
    CreateTerminalRequest, CreateTerminalResponse,
    TerminalOutputRequest, TerminalOutputResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    KillTerminalRequest, KillTerminalResponse,
    InitializeRequest, NewSessionRequest, 
    ByteStreams, ClientCapabilities, Implementation, ProtocolVersion,
    Client
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tokio::time::{timeout, Duration};
use anyhow::Result;

/// Example client that implements the Client trait
struct SimpleTerminalClient;

#[async_trait::async_trait]
impl Client for SimpleTerminalClient {
    // 实现其他必需的方法（这里简化处理）
    async fn read_text_file(&self, _args: agent_client_protocol::ReadTextFileRequest) 
        -> Result<agent_client_protocol::ReadTextFileResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::method_not_found())
    }
    
    async fn write_text_file(&self, _args: agent_client_protocol::WriteTextFileRequest) 
        -> Result<agent_client_protocol::WriteTextFileResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::method_not_found())
    }
    
    async fn create_terminal(&self, args: CreateTerminalRequest) 
        -> Result<CreateTerminalResponse, agent_client_protocol::Error> {
        // 这里模拟创建终端
        println!("📝 Received create_terminal request:");
        println!("   Command: {} {:?}", args.command, args.args);
        println!("   CWD: {:?}", args.cwd);
        
        // 模拟返回一个终端ID
        Ok(CreateTerminalResponse::new("term_example_123"))
    }
    
    async fn terminal_output(&self, args: TerminalOutputRequest) 
        -> Result<TerminalOutputResponse, agent_client_protocol::Error> {
        println!("📤 Received terminal_output request for: {}", args.terminal_id);
        
        // 模拟返回终端输出
        Ok(TerminalOutputResponse::new("Hello from terminal!", false))
    }
    
    async fn release_terminal(&self, args: ReleaseTerminalRequest) 
        -> Result<ReleaseTerminalResponse, agent_client_protocol::Error> {
        println!("🗑️  Received release_terminal request for: {}", args.terminal_id);
        Ok(ReleaseTerminalResponse::new())
    }
    
    async fn wait_for_terminal_exit(&self, args: WaitForTerminalExitRequest) 
        -> Result<WaitForTerminalExitResponse, agent_client_protocol::Error> {
        println!("⏳ Received wait_for_terminal_exit request for: {}", args.terminal_id);
        
        // 模拟返回退出状态
        use agent_client_protocol::TerminalExitStatus;
        Ok(WaitForTerminalExitResponse::new()
            .exit_status(TerminalExitStatus::new().exit_code(Some(0))))
    }
    
    async fn kill_terminal(&self, args: KillTerminalRequest) 
        -> Result<KillTerminalResponse, agent_client_protocol::Error> {
        println!("🔪 Received kill_terminal request for: {}", args.terminal_id);
        Ok(KillTerminalResponse::new())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 ACP Terminal Client Example");
    println!("================================\n");

    // 模拟：设置 I/O 流（实际应用中会连接到真实的 agent）
    // 这里我们用 stdio 作为示例
    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();

    // 1. 创建客户端连接
    let (conn, handle_io) = ClientSideConnection::new(
        SimpleTerminalClient,
        outgoing,
        incoming,
        |fut| tokio::task::spawn_local(fut),
    );

    // 在后台处理 I/O
    tokio::task::spawn_local(handle_io);

    // 2. 初始化连接
    println!("📡 Connecting to agent...");
    let init_response = conn.initialize(
        InitializeRequest::new(ProtocolVersion::V1)
            .client_info(Implementation::new("terminal-example", "1.0.0")
                .title("Terminal Example Client"))
            .client_capabilities(ClientCapabilities::new()
                .terminal(true))  // 声明支持 terminal
    ).await?;

    println!("✓ Connected to agent: {}", 
             init_response.agent_info.as_ref()
             .map(|i| &i.name).unwrap_or(&"unknown".to_string()));

    // 3. 创建会话
    let session_response = conn.new_session(
        NewSessionRequest::new(std::env::current_dir()?)
    ).await?;

    let session_id = session_response.session_id;
    println!("✓ Created session: {}", session_id);

    // 4. Terminal 请求示例
    
    // 示例 1: 创建终端执行命令
    println!("\n📝 Example 1: Create Terminal");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let create_request = CreateTerminalRequest::new(
        session_id.clone(), 
        "echo"
    )
    .args(vec!["Hello, Terminal!".to_string()])
    .cwd(Some("/tmp".to_string()))
    .output_byte_limit(Some(1024 * 1024)); // 1MB

    let create_response = conn.create_terminal(create_request).await?;
    let terminal_id = create_response.terminal_id;
    println!("✓ Terminal created with ID: {}", terminal_id);

    // 示例 2: 获取终端输出
    println!("\n📤 Example 2: Get Terminal Output");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let output_response = conn.terminal_output(
        TerminalOutputRequest::new(session_id.clone(), terminal_id.clone())
    ).await?;

    println!("Output: {}", output_response.output);
    if output_response.truncated {
        println!("⚠️  Output was truncated!");
    }
    if let Some(status) = output_response.exit_status {
        println!("Exit status: {:?}", status);
    }

    // 示例 3: 等待终端退出（带超时）
    println!("\n⏳ Example 3: Wait for Terminal Exit");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    match timeout(
        Duration::from_secs(10),
        conn.wait_for_terminal_exit(
            WaitForTerminalExitRequest::new(session_id.clone(), terminal_id.clone())
        )
    ).await {
        Ok(Ok(exit_response)) => {
            if let Some(status) = exit_response.exit_status {
                println!("✓ Terminal exited: {:?}", status);
            }
        }
        Ok(Err(e)) => {
            println!("⚠️  Error: {}", e);
        }
        Err(_) => {
            println!("⏰ Timeout - killing terminal");
            conn.kill_terminal(
                KillTerminalRequest::new(session_id.clone(), terminal_id.clone())
            ).await?;
        }
    }

    // 示例 4: 释放终端资源
    println!("\n🗑️  Example 4: Release Terminal");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    conn.release_terminal(
        ReleaseTerminalRequest::new(session_id, terminal_id)
    ).await?;
    
    println!("✓ Terminal resources released");

    println!("\n🎉 All examples completed successfully!");

    Ok(())
}
