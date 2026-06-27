//! Pure utilities and algorithms shared across Loom crates.
//!
//! This crate hosts dependency-light logic that multiple crates need
//! (text processing, string algorithms, etc.). It sits at the bottom of the
//! dependency graph alongside `loom-types`.

pub mod text;
