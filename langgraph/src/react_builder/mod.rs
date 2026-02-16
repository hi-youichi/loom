//! ReAct run context builder: builds checkpointer, store, runnable_config and tool source from config.
//!
//! This module provides a config-driven API for building ReAct agents without manually
//! constructing graphs, checkpointers, stores, or tool sources. Callers (e.g. CLIs)
//! hold a [`ReactBuildConfig`] and use builder functions to obtain a [`ReactRunContext`]
//! and/or a [`ReactRunner`](crate::ReactRunner) for [`run_react_graph`](crate::run_react_graph).
//!
//! # Interaction with external code
//!
//! This module is the **entry point** for config-driven ReAct from outside the library.
//!
//! | Direction | Description |
//! |-----------|-------------|
//! | **Callers → this module** | External code (e.g. CLIs, servers, or other crates) builds a [`ReactBuildConfig`] from their own sources (env via [`ReactBuildConfig::from_env`], CLI args, or a `RunConfig`), then calls [`build_react_run_context`], [`build_react_runner`], or [`build_react_runner_with_openai`]. They may pass an optional `Box<dyn LlmClient>` or let the library build the default OpenAI LLM from config. |
//! | **This module → callers** | The module only returns owned values: [`ReactRunContext`] and/or [`ReactRunner`]. No callbacks or handles back into the caller. Callers then use the returned [`ReactRunner`](crate::ReactRunner) with [`run_react_graph`](crate::run_react_graph) or [`run_react_graph_stream`](crate::run_react_graph_stream). |
//! | **This module → env / OS** | When building, it may spawn MCP subprocesses (e.g. mcp-remote) for tool source and open SQLite DBs for checkpointer/store. It does not read env itself during build (config is supplied by the caller). |
//!
//! So: **input** is config (and optional LLM); **output** is context and runner; **side effects** are subprocesses and DB access during build. Graph execution (LLM calls, tool calls) happens in [`run_react_graph`](crate::run_react_graph) after the caller invokes it with the built runner.
//!
//! # Workflow
//!
//! 1. **Load config**: Use [`ReactBuildConfig::from_env`] to load from environment variables
//!    (after `dotenv::dotenv().ok()` if using `.env`), or build config programmatically.
//! 2. **Build run context** (optional): Call [`build_react_run_context`] to get checkpointer,
//!    store, runnable_config, and tool_source. Use this when you need fine-grained control
//!    over the built resources.
//! 3. **Build runner**: Call [`build_react_runner`] or [`build_react_runner_with_openai`]
//!    to obtain a [`ReactRunner`](crate::ReactRunner). These functions internally call
//!    [`build_react_run_context`] when needed.
//! 4. **Run graph**: Use [`run_react_graph`](crate::run_react_graph) or
//!    [`run_react_graph_stream`](crate::run_react_graph_stream) to execute the agent.
//!
//! # Main types
//!
//! | Type | Description |
//! |------|-------------|
//! | [`ReactBuildConfig`] | Configuration for DB path, thread_id, user_id, system prompt, MCP/Exa settings, OpenAI and embedding keys. Use [`ReactBuildConfig::from_env`] to load from env. |
//! | [`ReactRunContext`] | Built run resources: checkpointer (short-term memory), store (long-term memory), runnable_config, and tool_source. Returned by [`build_react_run_context`]. |
//! | [`BuildRunnerError`] | Error when building the runner, e.g. missing API key ([`BuildRunnerError::NoLlm`]) or compilation failure. |
//!
//! # Main functions
//!
//! | Function | Description |
//! |----------|-------------|
//! | [`build_react_run_context`] | Builds checkpointer, store, runnable_config and tool_source from config. Returns [`ReactRunContext`]. |
//! | [`build_react_runner`] | Builds a [`ReactRunner`](crate::ReactRunner) from config and optional LLM. When `llm: None`, constructs default OpenAI LLM from config. |
//! | [`build_react_runner_with_openai`] | Convenience when you already have an [`OpenAIConfig`](async_openai::config::OpenAIConfig). Wraps [`build_react_runner`] with a pre-built OpenAI client. |
//!
//! # Environment variables
//!
//! When using [`ReactBuildConfig::from_env`], the following variables are read:
//!
//! | Variable | Description | Default |
//! |----------|-------------|---------|
//! | `DB_PATH` | SQLite database path for checkpointer/store | None (uses "memory.db" at build time) |
//! | `THREAD_ID` | Thread ID for short-term memory; enables checkpointer when set | None |
//! | `USER_ID` | User ID for long-term memory; enables store when set | None |
//! | `REACT_SYSTEM_PROMPT` | System prompt for the agent | None (library default) |
//! | `EXA_API_KEY` | Exa API key; enables MCP Exa when set | None |
//! | `MCP_EXA_URL` | Exa MCP server URL | `"https://mcp.exa.ai/mcp"` |
//! | `MCP_REMOTE_CMD` | Command for mcp-remote (stdio→HTTP bridge) | `"npx"` |
//! | `MCP_REMOTE_ARGS` | Args for mcp-remote | `"-y mcp-remote"` |
//! | `MCP_VERBOSE` / `VERBOSE` | Enable MCP verbose logging | `false` |
//! | `OPENAI_API_KEY` | OpenAI API key for default LLM | None |
//! | `OPENAI_BASE_URL` | OpenAI API base URL | None |
//! | `OPENAI_MODEL` | Model name (e.g. gpt-4o-mini) | None |
//! | `EMBEDDING_API_KEY` | Embedding API key for long-term memory | None |
//! | `EMBEDDING_API_BASE` | Embedding API base URL | None |
//! | `EMBEDDING_MODEL` | Embedding model (e.g. text-embedding-3-small) | None |
//!
//! # Feature requirements
//!
//! - **sqlite**: Required for `SqliteSaver` (checkpointer) and `SqliteStore`. Without it, checkpointer/store building will fail when `thread_id`/`user_id` are set.
//! - **mcp**: Required for MCP Exa tool source. Without it, Exa search tools will not be available even when `EXA_API_KEY` is set.
//! - **openai**: Required when using `build_react_runner(config, None, _)` to construct the default LLM from config.
//!
//! # Module structure
//!
//! - **config**: [`ReactBuildConfig`] and [`ReactBuildConfig::from_env`].
//! - **build**: [`build_react_run_context`], [`build_react_runner`], [`build_react_runner_with_openai`], [`ReactRunContext`], [`BuildRunnerError`].
//!
//! # Example: config-driven run
//!
//! ```rust,no_run
//! use langgraph::{build_react_runner, run_react_graph, ReactBuildConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = ReactBuildConfig::from_env();
//! let runner = build_react_runner(&config, None, false).await?;
//! let state = runner.invoke("Hello").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example: with explicit OpenAI config
//!
//! ```rust,no_run
//! use async_openai::config::OpenAIConfig;
//! use langgraph::{build_react_runner_with_openai, run_react_graph, ReactBuildConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = ReactBuildConfig::from_env();
//! let openai_config = OpenAIConfig::new().with_api_key(std::env::var("OPENAI_API_KEY").unwrap());
//! let runner = build_react_runner_with_openai(&config, openai_config, "gpt-4o-mini", false).await?;
//! let state = runner.invoke("Hello").await?;
//! # Ok(())
//! # }
//! ```

mod build;
mod config;

pub use build::{
    build_dup_runner, build_got_runner, build_react_run_context, build_react_runner,
    build_react_runner_with_openai, build_tot_runner, BuildRunnerError, ReactRunContext,
};
pub use config::ReactBuildConfig;
