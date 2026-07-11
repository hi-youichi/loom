//! Twitter/X API tools — native Loom implementations calling GetXAPI.
//!
//! These tools call the GetXAPI HTTP endpoints directly, without requiring
//! an MCP server. All tools share a single connection-pooled HTTP client.
//!
//! Requires `TWITTER_API_KEY` environment variable when used with ReactBuildConfig.

mod client;
mod tools;

pub use client::{TwitterClient, TwitterClientError};
pub use tools::{
    register_twitter_tools, register_twitter_tools_with_key, TwitterTool,
};
