//! Mock API test: advertise the **default full builtin tool set** (same shape as ReAct without
//! Exa/Twitter/memory/MCP/`invoke_agent`), YAML-merged specs like production, then one streaming
//! turn expecting a **`read`** tool call using a local mock HTTP server.
//!
//! No real API calls are made.

mod init_logging;

use std::sync::Arc;

use async_openai::config::OpenAIConfig;
use loom::llm::{ChatOpenAI, LlmClient, ToolCallDelta, ToolChoiceMode};
use loom::tool_source::{register_file_tools, ToolSource, YamlSpecToolSource};
use loom::tools::{
    AggregateToolSource, BashTool, BatchTool, LspTool, WebFetcherTool, TOOL_READ_FILE,
};
use loom::{Message, MessageChunk};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = stream.read(&mut tmp).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let header_end = pos + 4;
            let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let mut body = buf[header_end..].to_vec();
            while body.len() < content_length {
                let m = stream.read(&mut tmp).await.unwrap();
                if m == 0 {
                    break;
                }
                body.extend_from_slice(&tmp[..m]);
            }
            return String::from_utf8_lossy(&body[..content_length]).to_string();
        }
    }
    String::new()
}

#[allow(dead_code)]
async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    body: &str,
) {
    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    stream.write_all(resp.as_bytes()).await.unwrap();
}

async fn write_http_stream_response(
    stream: &mut tokio::net::TcpStream,
    body: &str,
) {
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{}",
        body
    );
    stream.write_all(resp.as_bytes()).await.unwrap();
}

async fn list_default_builtin_tools_merged_yaml(
    working_folder: &std::path::Path,
) -> Vec<loom::tool_source::ToolSpec> {
    let aggregate = Arc::new(AggregateToolSource::new());
    aggregate
        .register_async(Box::new(WebFetcherTool::new()))
        .await;
    aggregate.register_async(Box::new(BashTool::new())).await;
    #[cfg(windows)]
    {
        aggregate
            .register_async(Box::new(loom::tools::PowerShellTool::new()))
            .await;
    }
    register_file_tools(aggregate.as_ref(), working_folder, None)
        .unwrap_or_else(|e| panic!("register_file_tools: {e}"));
    aggregate.register_sync(Box::new(BatchTool::new(Arc::clone(&aggregate))));
    aggregate.register_sync(Box::new(LspTool::new()));

    let inner: Box<dyn ToolSource> = Box::new(aggregate);
    let wrapped = YamlSpecToolSource::wrap(inner)
        .await
        .unwrap_or_else(|e| panic!("YamlSpecToolSource::wrap: {e}"));
    wrapped
        .list_tools()
        .await
        .unwrap_or_else(|e| panic!("list_tools: {e}"))
}

#[tokio::test]
async fn mock_api_full_tool_list_invokes_read() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_addr = format!("http://{}", addr);

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _body = read_http_request(&mut stream).await;
        let sse_data = vec![
            r#"data: {"id":"chatcmpl-mock-read","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
            r#"data: {"id":"chatcmpl-mock-read","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_read_1","type":"function","function":{"name":"read"}}]},"finish_reason":null}]}"#,
            r#"data: {"id":"chatcmpl-mock-read","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\"probe.txt\"}"}}]},"finish_reason":null}]}"#,
            r#"data: {"id":"chatcmpl-mock-read","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            "data: [DONE]",
        ];
        let response = sse_data.join("\n\n") + "\n\n";
        write_http_stream_response(&mut stream, &response).await;
    });

    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("probe.txt"), "mock-test-marker").expect("write probe.txt");

    let tools = list_default_builtin_tools_merged_yaml(dir.path()).await;

    assert!(
        tools.len() >= 16,
        "expected full builtin tool list, got only {} tools",
        tools.len()
    );
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    for required in ["bash", "web_fetcher", TOOL_READ_FILE, "ls", "batch", "lsp"] {
        assert!(
            names.contains(&required),
            "tool {required:?} missing from listed tools: {names:?}"
        );
    }

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(server_addr);
    let llm = ChatOpenAI::with_config(config, "gpt-4o-mini")
        .with_tools(tools)
        .with_tool_choice(ToolChoiceMode::Required);

    let messages = vec![Message::user(format!(
        "The workspace contains a file named probe.txt. Use the `{}` tool to read it.",
        TOOL_READ_FILE
    ))];

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<MessageChunk>(64);
    let (tool_tx, mut tool_rx) = mpsc::channel::<ToolCallDelta>(64);
    let out = llm
        .invoke_stream_with_tool_delta(&messages, Some(chunk_tx), Some(tool_tx))
        .await
        .expect("mock API invoke_stream_with_tool_delta should succeed");

    while chunk_rx.recv().await.is_some() {}
    while tool_rx.recv().await.is_some() {}

    assert!(
        !out.tool_calls.is_empty(),
        "expected at least one tool call, got {}",
        out.tool_calls.len()
    );

    let read_call = out
        .tool_calls
        .iter()
        .find(|t| t.name == TOOL_READ_FILE)
        .unwrap_or_else(|| {
            panic!(
                "expected `{}` tool call, got names {:?}",
                TOOL_READ_FILE,
                out.tool_calls.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    let args: serde_json::Value =
        serde_json::from_str(read_call.arguments.trim()).unwrap_or_else(|e| {
            panic!(
                "read arguments should be JSON: {e}, raw: {:?}",
                read_call.arguments
            )
        });
    let path = args
        .get("path")
        .and_then(|p| p.as_str())
        .unwrap_or_default();
    assert!(
        path.contains("probe.txt"),
        "expected read path to reference probe.txt, got path: {:?}",
        path
    );
}
