use std::io::Write;
use std::process::{Command, Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn run_with_stdin(
    args: &[&str],
    input: &str,
    envs: Vec<(&str, &str)>,
) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_loom"));
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("failed to spawn loom");

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .expect("failed to write stdin");
    }

    child.wait_with_output().expect("failed to wait output")
}

async fn spawn_mock_llm() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                eprintln!("Mock server: accept failed");
                break;
            };
            
            eprintln!("Mock server: connection accepted");
            
            // Read the HTTP request with better handling
            let mut request_data = Vec::new();
            let mut buf = vec![0u8; 1024];
            
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => {
                        eprintln!("Mock server: connection closed by client");
                        break;
                    }
                    Ok(n) => {
                        request_data.extend_from_slice(&buf[..n]);
                        // Check if we have the complete HTTP headers
                        let request_str = String::from_utf8_lossy(&request_data);
                        if request_str.contains("\r\n\r\n") {
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("Mock server: read error: {}", e);
                        break;
                    }
                }
            }
            
            if request_data.is_empty() {
                eprintln!("Mock server: empty request");
                continue;
            }
            
            let request_str = String::from_utf8_lossy(&request_data);
            eprintln!("Mock server received request: {}", request_str.lines().next().unwrap_or("unknown"));
            
            let response = r#"{
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1,
                "model": "test",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "mock reply"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            }"#;
            
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                response.len(),
                response
            );
            
            eprintln!("Mock server sending response: {} bytes", resp.len());
            
            if let Err(e) = stream.write_all(resp.as_bytes()).await {
                eprintln!("Mock server write error: {}", e);
            } else {
                eprintln!("Mock server: response written successfully");
            }
            
            if let Err(e) = stream.flush().await {
                eprintln!("Mock server flush error: {}", e);
            } else {
                eprintln!("Mock server: response flushed successfully");
            }
            
            // Give time for the response to be read before closing
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            eprintln!("Mock server: closing connection");
        }
    });
    eprintln!("Mock server started on port {}", port);
    (format!("http://127.0.0.1:{}", port), handle)
}

#[test]
fn interactive_quit_immediately_exits_success() {
    let out = run_with_stdin(&["-i"], "quit\n", vec![]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Bye."));
}

#[test]
fn interactive_empty_line_then_quit_exits_success() {
    let out = run_with_stdin(&["-i"], "\nquit\n", vec![]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Bye."));
}

#[test]
fn interactive_initial_message_with_valid_working_folder_succeeds() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (mock_url, _handle) = rt.block_on(spawn_mock_llm());
    
    eprintln!("Mock URL: {}", mock_url);
    
    // Give the mock server more time to start accepting connections
    std::thread::sleep(std::time::Duration::from_millis(500));

    let out = run_with_stdin(
        &["-i", "-m", "hello", "--working-folder", "."],
        "quit\n",
        vec![
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", &mock_url),
            ("LOOM_TEST_MODE", "1"),
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        eprintln!("=== STDOUT ===\n{}", stdout);
        eprintln!("=== STDERR ===\n{}", stderr);
        eprintln!("=== STATUS ===\n{}", out.status);
        eprintln!("=== MOCK URL ===\n{}", mock_url);
    }
    assert!(out.status.success());
    assert!(stdout.contains("Bye."));
}
