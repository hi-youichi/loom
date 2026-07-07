//! E2E test suite crate root (plan 026).
//!
//! Phase 1 only declares `common`; Phase 2 adds Mega-specific helpers and
//! Phase 3 adds `authenticate.rs` / `session_load.rs` / `reload.rs` /
//! `terminal.rs` / `llm_error.rs` micro cases.

pub mod common;

// Micro tests (Phase 3)
mod reload;
