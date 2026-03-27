//! Optional live LLM API test: **streaming** `invoke_stream_with_tool_delta` with `bash` tool
//! advertised, assert tool call shape. Loads `.env` from the workspace root (`../.env` from
//! this crate) then the current directory.
//!
//! Note: [`MessageChunk`] is only sent for **text** deltas on `chunk_tx`. Tool-call fragments
//! are sent on `tool_delta_tx`; tool-only replies may legitimately produce **zero** `MessageChunk`s.
//!
//! Does not run ActNode or execute shell commands — only verifies the provider returns a
//! `bash` tool call whose JSON arguments include a list-directory style command.
//!
//! **Run** (charges API usage):
//!
//! ```bash
//! # .env at repo root (dev/.env) may contain OPENAI_API_KEY, OPENAI_BASE_URL, OPENAI_MODEL
//! cargo test -p loom live_api_invoke_returns_bash_list_dir_tool_call -- --ignored --nocapture
//! ```
//!
//! The test is `#[ignore]` so `cargo test` does not hit the network even when `OPENAI_API_KEY` is set.
//! Use `cargo test -p loom -- --ignored` to run ignored tests only when you intend to.

mod init_logging;

use async_openai::config::OpenAIConfig;
use loom::llm::{ChatOpenAI, LlmClient, ToolCallDelta, ToolChoiceMode};
use loom::tools::{BashTool, Tool};
use loom::{Message, MessageChunk};
use tokio::sync::mpsc;

/// Load `.env`: prefer workspace root next to this package, then default discovery.
fn load_test_env() {
    let workspace_dotenv =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(".env");
    let _ = dotenv::from_filename(workspace_dotenv);
    let _ = dotenv::dotenv();
}

/// Build [`OpenAIConfig`] with explicit API key and optional base URL from the environment.
fn openai_config_from_env() -> Result<OpenAIConfig, std::env::VarError> {
    let api_key = std::env::var("OPENAI_API_KEY")?;
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(std::env::VarError::NotPresent);
    }
    let mut config = OpenAIConfig::new().with_api_key(api_key.to_string());
    if let Ok(base) = std::env::var("OPENAI_BASE_URL").or_else(|_| std::env::var("OPENAI_API_BASE"))
    {
        let base = base.trim_end_matches('/').to_string();
        if !base.is_empty() {
            config = config.with_api_base(base);
        }
    }
    Ok(config)
}

#[tokio::test]
#[ignore = "live API; run with: cargo test -p loom live_api_invoke_returns_bash_list_dir_tool_call -- --ignored --nocapture"]
async fn live_api_invoke_returns_bash_list_dir_tool_call() {
    load_test_env();

    let api_key_ok = std::env::var("OPENAI_API_KEY")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !api_key_ok {
        eprintln!(
            "skipping live_api_invoke_returns_bash_list_dir_tool_call (set OPENAI_API_KEY in .env or env)"
        );
        return;
    }

    let config = match openai_config_from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping live API test (OPENAI_API_KEY): {e}");
            return;
        }
    };

    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
    let llm = ChatOpenAI::with_config(config, model)
        .with_tools(vec![BashTool::new().spec()])
        .with_tool_choice(ToolChoiceMode::Required);

    let messages = vec![Message::user(
        "Use the bash tool exactly once. Run a command that lists file names in the OS temporary directory. \
         On Unix the command should include the substring `ls`. \
         Do not add a normal assistant reply; only the tool call.",
    )];

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<MessageChunk>(64);
    let (tool_tx, mut tool_rx) = mpsc::channel::<ToolCallDelta>(64);
    let out = llm
        .invoke_stream_with_tool_delta(&messages, Some(chunk_tx), Some(tool_tx))
        .await
        .unwrap_or_else(|e| panic!("live API invoke_stream_with_tool_delta failed: {e}"));

    let mut content_chunks = 0usize;
    while chunk_rx.recv().await.is_some() {
        content_chunks += 1;
    }
    let mut tool_deltas = 0usize;
    while tool_rx.recv().await.is_some() {
        tool_deltas += 1;
    }

    assert!(
        content_chunks > 0 || tool_deltas > 0 || !out.tool_calls.is_empty(),
        "expected text chunks on chunk_tx and/or tool deltas on tool_delta_tx and/or assembled tool_calls; \
         got content_chunks={content_chunks} tool_deltas={tool_deltas} tool_calls={}",
        out.tool_calls.len()
    );

    assert!(
        !out.tool_calls.is_empty(),
        "expected at least one tool call, got {} tool_calls (content len {})",
        out.tool_calls.len(),
        out.content.len()
    );

    let bash = out
        .tool_calls
        .iter()
        .find(|t| t.name == "bash")
        .unwrap_or_else(|| {
            panic!(
                "expected a bash tool call, got: {:?}",
                out.tool_calls
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
            )
        });

    let args: serde_json::Value =
        serde_json::from_str(bash.arguments.trim()).unwrap_or_else(|e| {
            panic!(
                "bash arguments should be JSON: {e}, raw: {:?}",
                bash.arguments
            )
        });

    let cmd = args
        .get("command")
        .and_then(|c| c.as_str())
        .unwrap_or_default();
    let lower = cmd.to_lowercase();
    assert!(
        lower.contains("ls")
            || lower.contains("dir ")
            || lower.contains("\\dir")
            || lower.contains("get-childitem"),
        "expected a list-directory shell command, got command: {:?}",
        cmd
    );
}
