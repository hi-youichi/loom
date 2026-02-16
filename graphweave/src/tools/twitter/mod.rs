//! Twitter tools: search tweets via twitterapi.io.
//!
//! Requires `TWITTER_API_KEY` environment variable when used with ReactBuildConfig.
//! API docs: https://docs.twitterapi.io/api-reference/endpoint/tweet_advanced_search

mod search;

pub use search::{TwitterSearchTool, TOOL_TWITTER_SEARCH};
