use async_openai::config::OpenAIConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::llm::{LlmClient, ToolChoiceMode};
use crate::message::Message;
use crate::tool_source::ToolSpec;

use super::ChatOpenAI;

fn env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

async fn read_http_request(stream: &mut TcpStream) -> String {
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

async fn write_http_response(stream: &mut TcpStream, status: &str, body: &str) {
    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    stream.write_all(resp.as_bytes()).await.unwrap();
}

async fn write_http_stream_response(stream: &mut TcpStream, body: &str) {
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{}",
        body
    );
    stream.write_all(resp.as_bytes()).await.unwrap();
}

#[test]
fn chat_openai_new_creates_client() {
    let _ = ChatOpenAI::new("gpt-4");
    let _ = ChatOpenAI::new("gpt-4o-mini");
}

#[test]
fn chat_openai_with_config_creates_client() {
    let config = OpenAIConfig::new().with_api_key("test-key");
    let _ = ChatOpenAI::with_config(config, "gpt-4");
}

#[test]
fn chat_openai_with_tools_and_temperature_builder() {
    let tools = vec![ToolSpec {
        name: "get_time".into(),
        description: None,
        input_schema: serde_json::json!({}),
        output_hint: None,
    }];
    let _ = ChatOpenAI::new("gpt-4")
        .with_tools(tools)
        .with_temperature(0.5f32);
}

#[test]
fn chat_completions_url_uses_env_variants() {
    let _guard = env_lock().lock().unwrap();
    std::env::set_var("OPENAI_BASE_URL", "https://example.com");
    assert_eq!(
        ChatOpenAI::chat_completions_url(),
        "https://example.com/v1/chat/completions"
    );
    std::env::remove_var("OPENAI_BASE_URL");

    std::env::set_var("OPENAI_API_BASE", "https://example.com/v1");
    assert_eq!(
        ChatOpenAI::chat_completions_url(),
        "https://example.com/v1/chat/completions"
    );
    std::env::remove_var("OPENAI_API_BASE");
}

#[tokio::test]
async fn invoke_with_unreachable_base_returns_error() {
    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base("https://127.0.0.1:1");
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
    let messages = [Message::user("Hello")];

    let result = client.invoke(&messages).await;

    assert!(
        result.is_err(),
        "invoke against unreachable base should return Err"
    );
}

#[tokio::test]
async fn invoke_stream_with_unreachable_base_returns_error() {
    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base("http://127.0.0.1:1/v1");
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
    let (chunk_tx, _chunk_rx) = mpsc::channel(8);
    let err = client
        .invoke_stream(&[Message::user("hello")], Some(chunk_tx))
        .await
        .err()
        .unwrap();
    assert!(err.to_string().contains("OpenAI stream error"));
}

#[tokio::test]
async fn invoke_does_not_retry_non_retryable_400_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut attempts = 0;
        if let Ok((mut stream, _)) = listener.accept().await {
            attempts += 1;
            let _ = read_http_request(&mut stream).await;
            write_http_response(
                &mut stream,
                "400 Bad Request",
                r#"{"error":{"message":"messages with role 'tool' must be a response to a preceding message with 'tool_calls'"}}"#,
            )
            .await;
        }
        attempts
    });

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(format!("http://{addr}/v1"));
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini");

    let err = client.invoke(&[Message::user("hello")]).await.err().unwrap();
    assert!(err.to_string().contains("OpenAI API error"));
    assert_eq!(server.await.unwrap(), 1);
}

#[tokio::test]
async fn invoke_retries_retryable_500_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut attempts = 0;
        while attempts < 2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            attempts += 1;
            let _ = read_http_request(&mut stream).await;
            if attempts == 1 {
                write_http_response(
                    &mut stream,
                    "500 Internal Server Error",
                    r#"{"error":{"message":"temporary upstream failure"}}"#,
                )
                .await;
            } else {
                write_http_response(
                    &mut stream,
                    "200 OK",
                    r#"{"id":"chatcmpl-test","object":"chat.completion","created":0,"model":"gpt-4o-mini","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
                )
                .await;
            }
        }
        attempts
    });

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(format!("http://{addr}/v1"));
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini");

    let response = client.invoke(&[Message::user("hello")]).await.unwrap();
    assert_eq!(response.content, "ok");
    assert_eq!(server.await.unwrap(), 2);
}


#[tokio::test]
async fn invoke_stream_with_none_channel_delegates_to_invoke() {
    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base("https://127.0.0.1:1");
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
    let messages = [Message::user("Hi")];

    let res_invoke = client.invoke(&messages).await;
    let res_stream = client.invoke_stream(&messages, None).await;

    assert!(res_invoke.is_err());
    assert!(res_stream.is_err());
}

#[tokio::test]
async fn invoke_and_invoke_stream_none_channel_succeed_with_mock_server() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let body = read_http_request(&mut stream).await;
            let req: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            assert!(req.get("messages").is_some());
            let response = serde_json::json!({
                "id":"chatcmpl-1",
                "object":"chat.completion",
                "created": 1,
                "model":"gpt-4o-mini",
                "choices":[
                    {
                        "index":0,
                        "message":{
                            "role":"assistant",
                            "content":"hello",
                            "tool_calls":[
                                {
                                    "id":"call_1",
                                    "type":"function",
                                    "function":{"name":"get_time","arguments":"{}"}
                                }
                            ]
                        },
                        "finish_reason":"stop"
                    }
                ],
                "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}
            })
            .to_string();
            write_http_response(&mut stream, "200 OK", &response).await;
        }
    });

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(format!("http://{}", addr));
    let tools = vec![ToolSpec {
        name: "get_time".into(),
        description: Some("time".into()),
        input_schema: serde_json::json!({"type":"object"}),
        output_hint: None,
    }];
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini")
        .with_tools(tools)
        .with_temperature(0.2)
        .with_tool_choice(ToolChoiceMode::Required);
    let messages = [Message::user("hello")];
    let res = client.invoke(&messages).await.unwrap();
    assert_eq!(res.content, "hello");
    assert_eq!(res.tool_calls.len(), 1);
    assert_eq!(res.usage.unwrap().total_tokens, 2);

    let res_stream = client.invoke_stream(&messages, None).await.unwrap();
    assert_eq!(res_stream.content, "hello");
    assert_eq!(res_stream.tool_calls.len(), 1);
}

#[tokio::test]
async fn invoke_returns_error_when_choices_missing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_http_request(&mut stream).await;
        let response = serde_json::json!({
            "id":"chatcmpl-2",
            "object":"chat.completion",
            "created": 1,
            "model":"gpt-4o-mini",
            "choices":[],
            "usage":{"prompt_tokens":1,"completion_tokens":0,"total_tokens":1}
        })
        .to_string();
        write_http_response(&mut stream, "200 OK", &response).await;
    });

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(format!("http://{}", addr));
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
    let err = match client.invoke(&[Message::user("x")]).await {
        Ok(_) => panic!("expected no-choices error"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("no choices"));
}

#[tokio::test]
async fn invoke_with_mock_api_returns_ok() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_http_request(&mut stream).await;
        let response = serde_json::json!({
            "id":"chatcmpl-mock",
            "object":"chat.completion",
            "created": 1,
            "model":"gpt-4o-mini",
            "choices":[
                {
                    "index":0,
                    "message":{
                        "role":"assistant",
                        "content":"ok"
                    },
                    "finish_reason":"stop"
                }
            ],
            "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}
        })
        .to_string();
        write_http_response(&mut stream, "200 OK", &response).await;
    });

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(format!("http://{}", addr));
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
    let messages = [Message::user("Say exactly: ok")];

    let result = client.invoke(&messages).await;

    let response = result.expect("invoke with mock API should succeed");
    assert!(
        !response.content.is_empty() || !response.tool_calls.is_empty(),
        "response should have content or tool_calls"
    );
}

    #[allow(clippy::useless_vec)]
    #[tokio::test]
    async fn invoke_stream_with_mock_api_returns_ok() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_http_request(&mut stream).await;
        let sse_data = vec![
            r#"data: {"id":"chatcmpl-mock-stream","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
            r#"data: {"id":"chatcmpl-mock-stream","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]}"#,
            r#"data: {"id":"chatcmpl-mock-stream","object":"chat.completion.chunk","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
            "data: [DONE]",
        ];
        let response = sse_data.join("\n\n") + "\n\n";
        write_http_stream_response(&mut stream, &response).await;
    });

    let config = OpenAIConfig::new()
        .with_api_key("test-key")
        .with_api_base(format!("http://{}", addr));
    let client = ChatOpenAI::with_config(config, "gpt-4o-mini");
    let messages = [Message::user("Say exactly: ok")];
    let (tx, mut rx) = mpsc::channel(16);

    let result = client.invoke_stream(&messages, Some(tx)).await;

    let response = result.expect("invoke_stream with mock API should succeed");
    assert!(
        !response.content.is_empty() || !response.tool_calls.is_empty(),
        "response should have content or tool_calls"
    );

    let mut chunks = 0u32;
    while rx.try_recv().is_ok() {
        chunks += 1;
    }
    assert!(chunks > 0, "should receive at least one stream chunk");
}
