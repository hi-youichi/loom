//! E2E test common utilities for `loom-acp` (plan 026).
//!
//! Modules here are imported by both `tests/e2e_mega.rs` (via `#[path]`) and
//! the micro cases under `tests/e2e/`. Keep this surface area small and
//! focused on JSON-RPC fixture plumbing — no business logic.

pub mod env;
pub mod harness;
pub mod jsonrpc;
pub mod mock_llm;
pub mod permissions;

#[allow(unused_imports)]
pub use env::{binary_path, with_loom_home, TestEnv};
#[allow(unused_imports)]
pub use harness::AcpTestHarness;
#[allow(unused_imports)]
pub use jsonrpc::{JsonRpcClient, JsonRpcFrame, ReverseRpcKind, SessionNotification};
#[allow(unused_imports)]
pub use mock_llm::MockLlmServer;
#[allow(unused_imports)]
pub use permissions::{PermissionPolicy, ReverseRpcResponder};