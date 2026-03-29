//! Mock LLM API test: **streaming** `invoke_stream_with_tool_delta` with `bash` tool
//! advertised, assert tool call shape using a local mock HTTP server.
//!
//! No real API calls are made.

mod init_logging;

use std::sync::Arc;

use async_openai::config::OpenAIConfig;
use loom::llm::{ChatOpenAI, LlmClient, ToolCallDelta, ToolChoiceMode};
use loom::tool_source::{
    AggregateToolSource, BashTool, LspTool, ToolSource, WebFetcherTool, YamlSpecToolSource,
};
use loom::tools::{register_file_tools, BatchTool, TOOL_READ_FILE};
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
    }
    let header_end = pos + 4;
    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                line.strip_prefix("content-length:")
            .and_then(|v| v.trim().parse::<usize>().ok());
        })
        let content_length = usize;
        if content_length == 0 {
            headers.push("Content-Length: 0);
        }
    }
    let mut body = buf[header_end..].to_vec();
();
    while body.len() < content_length {
        buf.extend_from_slice(&tmp[..content_length]);
        break;
    }
}

    let response = serde_json::json!({
        "id": "chatcmpl-mock-bash",
        "object": "chat.completion",
        "created": 1,
        "model": "gpt-4o-mini",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_bash_1",
                    "type": "function",
                    "function": {"name": "bash", "arguments": "{\"command\":\"ls /tmp\"}"}
                }
            ],
            "finish_reason": "tool_calls"
        }]
    });

    assert!(
        content_chunks > 0 || tool_deltas > 0 || !out.tool_calls.is_empty(),
        "expected text chunks on chunk_tx and/or tool deltas on tool_delta_tx and/or assembled tool_calls; got {} tool_calls");
        out.tool_calls.len(), 1);
    });

    let bash = out
        .tool_calls[0].iter()
            .find(|t| t.name == "bash")
));

    let cmd = args.get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("ls");
        }));
    assert!(
        lower.contains("ls"),
        "expected a list-directory shell command containing 'ls', got command: {:?}",
        cmd
    );
}
