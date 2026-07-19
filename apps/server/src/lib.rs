//! loom-server library surface.
//!
//! Implementation note: the binary (`main.rs`) and the integration tests
//! (`tests/*.rs`) both link against this library so we can exercise the
//! route surface from `loom-server`'s own test suite without spawning
//! the actual `axum::serve` binary in P3.25.

pub mod agent_runner;
pub mod auth;
pub mod handlers;
pub mod location;
pub mod pty;
pub mod routes;
pub mod sse;
pub mod state;
pub mod translator;
pub mod acp_hub;
