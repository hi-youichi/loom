//! Pure utilities and algorithms shared across anureo crates.
//!
//! This crate hosts dependency-light logic that multiple crates need
//! (text processing, string algorithms, etc.). It sits at the bottom of the
//! dependency graph alongside `anureo-stream / tool-core / agent-core`.

pub mod text;
