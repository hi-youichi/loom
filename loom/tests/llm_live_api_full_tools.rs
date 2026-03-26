//! Live API test: advertise the **default full builtin tool set** (same shape as ReAct without
//! Exa/Twitter/memory/MCP/`invoke_agent`), YAML-merged specs like production, then one streaming
//! turn expecting a **`read`** tool call for a known file under a temp workspace.
//!
//! API-only: does not run ActNode. Omits optional tools that need API keys or full
//! [`loom::agent::react::ReactBuildConfig`].
//!
//! ```bash
//! cargo test -p loom live_api_full_tool_list_invokes_read -- --ignored --nocapture
//! ```

mod init_logging;

use std::sync::Arc;

use async_openai::config::OpenAIConfig;
use loom::llm::{ChatOpenAI, LlmClient, ToolCallDelta, ToolChoiceMode};
use loom::tool_source::{register_file_tools, ToolSource, YamlSpecToolSource};
use loom::tools::{
    AggregateToolSource, BashTool, BatchTool, LspTool, WebFetcherTool, TOOL_READ_FILE,
};
#[cfg(windows)]
use loom::tools::PowerShellTool;
use loom::{Message, MessageChunk};
use tokio::sync::mpsc;

fn load_test_env() {
    let workspace_dotenv =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(".env");
    let _ = dotenv::from_filename(workspace_dotenv);
    let _ = dotenv::dotenv();
}

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

/// Mirrors [`loom::agent::react::build::tool_source`] for a normal workspace (minus optional extras).
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
            .register_async(Box::new(PowerShellTool::new()))
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
#[ignore = "live API; run with: cargo test -p loom live_api_full_tool_list_invokes_read -- --ignored --nocapture"]
async fn live_api_full_tool_list_invokes_read() {
    load_test_env();

    let api_key_ok = std::env::var("OPENAI_API_KEY")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !api_key_ok {
        eprintln!("skipping live_api_full_tool_list_invokes_read (set OPENAI_API_KEY in .env or env)");
        return;
    }

    let config = match openai_config_from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping live API test (OPENAI_API_KEY): {e}");
            return;
        }
    };

    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("probe.txt"), "live-test-marker").expect("write probe.txt");

    let tools = list_default_builtin_tools_merged_yaml(dir.path()).await;

    assert!(
        tools.len() >= 16,
        "expected full builtin tool list, got only {} tools",
        tools.len()
    );
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    for required in [
        "bash",
        "web_fetcher",
        TOOL_READ_FILE,
        "ls",
        "batch",
        "lsp",
    ] {
        assert!(
            names.contains(&required),
            "tool {required:?} missing from listed tools: {names:?}"
        );
    }

    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
    let llm = ChatOpenAI::with_config(config, model)
        .with_tools(tools)
        .with_tool_choice(ToolChoiceMode::Required);

    let messages = vec![Message::user(format!(
        "The agent workspace root contains a file named probe.txt (plain text). \
         Use exactly one tool call: the `{read}` tool to read that file. \
         Use path \"probe.txt\" (relative path). Do not use bash. No assistant prose — tool call only.",
        read = TOOL_READ_FILE
    ))];

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<MessageChunk>(64);
    let (tool_tx, mut tool_rx) = mpsc::channel::<ToolCallDelta>(64);
    let out = llm
        .invoke_stream_with_tool_delta(&messages, Some(chunk_tx), Some(tool_tx))
        .await
        .unwrap_or_else(|e| panic!("live API invoke_stream_with_tool_delta failed: {e}"));

    eprintln!("--- llm_live_api_full_tools: stream message chunks ---");
    let mut content_chunks = 0usize;
    while let Some(chunk) = chunk_rx.recv().await {
        eprintln!("  chunk[{}] kind={:?} text={:?}", content_chunks, chunk.kind, chunk.content);
        content_chunks += 1;
    }

    eprintln!("--- llm_live_api_full_tools: stream tool_call deltas ---");
    let mut tool_deltas = 0usize;
    while let Some(delta) = tool_rx.recv().await {
        eprintln!("  delta[{}] {:?}", tool_deltas, delta);
        tool_deltas += 1;
    }

    eprintln!("--- llm_live_api_full_tools: assembled LlmResponse ---");
    eprintln!("  content ({} chars): {:?}", out.content.len(), out.content);
    if let Some(ref rc) = out.reasoning_content {
        eprintln!("  reasoning_content ({} chars): {:?}", rc.len(), rc);
    } else {
        eprintln!("  reasoning_content: None");
    }
    eprintln!("  usage: {:?}", out.usage);
    eprintln!("  tool_calls ({}):", out.tool_calls.len());
    for (i, t) in out.tool_calls.iter().enumerate() {
        eprintln!(
            "    [{}] name={:?} id={:?} arguments={:?}",
            i, t.name, t.id, t.arguments
        );
    }

    assert!(
        content_chunks > 0 || tool_deltas > 0 || !out.tool_calls.is_empty(),
        "expected stream activity or assembled tool_calls; content_chunks={content_chunks} tool_deltas={tool_deltas} n_tools={}",
        out.tool_calls.len()
    );

    assert!(
        !out.tool_calls.is_empty(),
        "expected at least one tool call, got {} (content len {})",
        out.tool_calls.len(),
        out.content.len()
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
