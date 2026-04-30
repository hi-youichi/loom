//! Response sending utilities.
//!
//! The original `send_response` (direct WebSocket) has been replaced by
//! `send_response_to_sink` in `connection.rs` which uses the shared `SplitSink`.
