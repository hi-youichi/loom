use agent_client_protocol::{
    Client, ClientSideConnection, 
    CreateTerminalRequest, CreateTerminalResponse,
    TerminalOutputRequest, TerminalOutputResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    KillTerminalRequest, KillTerminalResponse,
    InitializeRequest, NewSessionRequest, 
    ByteStreams, ClientCapabilities, Implementation, ProtocolVersion,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tokio::time::{timeout, Duration};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. 启动 agent 进程（这里假设 agent 支持 terminal）
    let mut child = tokio::process::Command::new("your-agent-binary")
        .arg("--acp")
        .stdout(std::process::Stdio::piped())
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    let outgoing = child.stdin.take().unwrap().compat_write();
    let incoming = child.stdout.take().unwrap().compat();

    // 2. 创建客户端连接
    let (conn, handle_io) = ClientSideConnection::new(
        TerminalClient {},
        outgoing,
        incoming,
        |fut| tokio::task::spawn(fut),
    );

    tokio::spawn(handle_io);

    // 3. 初始化连接
    let init_response = conn.initialize(
        InitializeRequest::new(ProtocolVersion::V1)
            .client_info(Implementation::new("terminal-example", "1.0.0")
                .title("Terminal Example Client"))
            .client_capabilities(ClientCapabilities::new()
                .terminal(true))  // 声明支持 terminal
    ).await?;

    println!("✓ Initialized with agent: {}", 
             init_response.agent_info.as_ref()
             .map(|i| &i.name).unwrap_or(&"unknown".to_string()));

    // 4. 创建会话
    let session_response = conn.new_session(
        NewSessionRequest::new(std::env::current_dir()?)
    ).await?;

    let session_id = session_response.session_id;
    println!("✓ Created session: {}", session_id);

    // 5. 创建终端执行命令
    let create_request = CreateTerminalRequest::new(session_id.clone(), "ls")
        .args(vec!["-la".to_string(), "/tmp".to_string()])
        .cwd(Some("/tmp".to_string()))
        .env(vec![])  // 可以设置环境变量
        .output_byte_limit(Some(1024 * 1024)); // 1MB 输出限制

    println!("🚀 Creating terminal to run: ls -la /tmp");
    let create_response = conn.create_terminal(create_request).await?;
    let terminal_id = create_response.terminal_id;
    println!("✓ Terminal created: {}", terminal_id);

    // 6. 等待命令完成（带超时）
    println!("⏳ Waiting for terminal to complete...");
    match timeout(
        Duration::from_secs(30),
        conn.wait_for_terminal_exit(
            WaitForTerminalExitRequest::new(session_id.clone(), terminal_id.clone())
        )
    ).await {
        Ok(Ok(exit_response)) => {
            if let Some(status) = exit_response.exit_status {
                if let Some(code) = status.exit_code {
                    println!("✓ Terminal exited with code: {}", code);
                } else if let Some(signal) = status.signal {
                    println!("✓ Terminal terminated by signal: {}", signal);
                }
            }
        }
        Ok(Err(e)) => {
            println!("⚠️  Error waiting for exit: {}", e);
            // 超时则杀死进程
            println!("🔪 Killing terminal due to error...");
            conn.kill_terminal(
                KillTerminalRequest::new(session_id.clone(), terminal_id.clone())
            ).await?;
        }
        Err(_) => {
            println!("⏰ Timeout! Killing terminal...");
            conn.kill_terminal(
                KillTerminalRequest::new(session_id.clone(), terminal_id.clone())
            ).await?;
        }
    }

    // 7. 获取终端输出
    println!("📤 Getting terminal output...");
    let output_response = conn.terminal_output(
        TerminalOutputRequest::new(session_id.clone(), terminal_id.clone())
    ).await?;

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Terminal Output:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("{}", output_response.output);
    
    if output_response.truncated {
        println!("⚠️  Output was truncated!");
    }
    
    if let Some(status) = output_response.exit_status {
        if let Some(code) = status.exit_code {
            println!("Exit code: {}", code);
        } else if let Some(signal) = status.signal {
            println!("Terminated by signal: {}", signal);
        }
    }

    // 8. 释放终端资源（重要！）
    println!("🗑️  Releasing terminal resources...");
    conn.release_terminal(
        ReleaseTerminalRequest::new(session_id, terminal_id)
    ).await?;
    println!("✓ Terminal released");

    Ok(())
}

// 实现 Client trait 来处理来自 agent 的请求
struct TerminalClient;

#[async_trait::async_trait]
impl Client for TerminalClient {
    // 这里实现其他必需的方法...
    // 为了示例简洁，我们只实现基本结构
}