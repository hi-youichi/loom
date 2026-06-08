//! # Loom
//!
//! Run orchestration and shared types for building stateful AI agents.
//!
//! This crate provides the `cli_run` module which contains profile loading,
//! helve config building, and model resolution logic.
//!
//! All core types are now in their own crates — import them directly:
//!
//! - `loom_graph` — `StateGraph`, `CompiledStateGraph`, `Node`, `Next`, `Agent`, `channels`, `managed`
//! - `loom_llm` — `LlmClient`, `ChatOpenAI`, `MockLlm`, `Message`, `ToolCall`, etc.
//! - `loom_types` — `ReActState`, `ToolResult`, `ToolCall`, `ModelConfig`, approval types
//! - `loom_tools` — `ToolSource`, `ToolSpec`, `McpToolSource`, `BashTool`, etc.
//! - `loom_memory` — `Checkpointer`, `Store`, `MemorySaver`, `SqliteSaver`
//! - `loom_stream` — `StreamEvent`, `StreamMode`, `StreamWriter`
//! - `loom_helve` — `HelveConfig`, `assemble_system_prompt`, approval policy
//! - `loom_tier` — tier resolution, model registry, LLM factory
//! - `loom_cache` — `Cache`, `InMemoryCache`
//! - `loom_compress` — context compression / compaction
//! - `loom_protocol` — WebSocket message types, streaming protocol
//! - `loom_pregel` — low-level Pregel graph runtime
//! - `loom_commands` — slash command parsing and execution
//! - `loom_model_spec` — `ModelSpec`, resolvers
//! - `loom_background_review` — background review system
//! - `loom_stream_display` — stream event display/rendering
//! - `loom_worktree` — git worktree isolation
//! - `loom_lsp` — LSP integration
//! - `loom_cli_types` — `RunOptions`, `RunCmd`, `RunCompletion`, `AnyStreamEvent`
//! - `loom_react_config` — `ReactBuildConfig`, profile types

pub mod cli_run;

/// Global lock for tests that modify `LOOM_HOME` or `OPENAI_BASE_URL` env vars.
/// Use in any test that sets/removes these env vars to prevent data races.
#[cfg(test)]
pub fn env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// When running `cargo test -p loom`, initializes tracing from `RUST_LOG` so that
/// unit tests in `src/**` (e.g. `openai.rs` `mod tests`) can print logs with `--nocapture`.
#[cfg(test)]
mod test_logging {
    use ctor::ctor;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::Layer;

    #[ctor]
    fn init() {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
        let _ = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_test_writer()
                    .with_filter(filter),
            )
            .try_init();
    }
}
