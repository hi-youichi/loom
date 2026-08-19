#![cfg_attr(coverage, feature(coverage_attribute))]
//! `loom-git`: typed git facade for Loom.
//!
//! Layering (see docs/design/git2-migration.md §4):
//! - `types`    — typed structs matching the extension JSON contract byte-for-byte
//! - `error`    — `GitError` with kind classification
//! - `backend`  — `GitBackend` trait; default bodies return `Unsupported`
//! - `cli`      — `CliBackend` over the git binary (parity baseline + fallback)
//! - `facade`   — backend selection (`LOOM_GIT_BACKEND`) + method-level delegation

pub mod backend;
pub mod cli;
pub mod error;
pub mod facade;
pub mod git2_backend;
pub mod git2_ops;
pub mod types;

pub use backend::{GitBackend, LogQuery};
pub use cli::CliBackend;
pub use error::{GitError, GitErrorKind};
pub use facade::{backend_kind, run_apply_raw, run_raw, BackendKind};
pub use git2_backend::Git2Backend;
