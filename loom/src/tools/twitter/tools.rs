//! Twitter API tools — native Loom tool implementations calling GetXAPI.
//!
//! These tools call the GetXAPI HTTP endpoints directly, without requiring
//! an MCP server. All tools share a single connection-pooled HTTP client.
//!
//! # Architecture
//!
//! - `TwitterOp` enum — one variant per API endpoint
//! - `TwitterTool` struct — holds op, spec, and shared client
//! - `all_tool_defs()` — declarative catalog of all tools
//! - Handler functions — one per endpoint, parse args → call client

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::registry::ToolRegistryLocked;
use crate::tools::Tool;

use super::client::{TwitterClient, TwitterClientError};

// =========================================================================
// Argument helpers
// =========================================================================

fn req_str(args: &Value, key: &str) -> Result<String, TwitterClientError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| client_input_error(format!("missing or empty required argument: '{key}'")))
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn req_f64(args: &Value, key: &str) -> Result<f64, TwitterClientError> {
    args.get(key)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| client_input_error(format!("missing or invalid argument: '{key}'")))
}

fn opt_int(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
}

fn client_input_error(msg: String) -> TwitterClientError {
    TwitterClientError::Api { code: "400".to_string(), msg }
}

/// Resolve login_cookie from args or env.
fn cookie(args: &Value) -> String {
    opt_str(args, "login_cookie")
        .or_else(|| std::env::var("TWITTER_LOGIN_COOKIE").ok().filter(|s| !s.is_empty()))
        .unwrap_or_default()
}

/// Inject login_cookies + proxy into a write-operation body.
fn write_body(cookie_val: &str, mut body: Value) -> Value {
    body["login_cookies"] = json!(cookie_val);
    body["proxy"] = json!(std::env::var("TWITTER_PROXY").unwrap_or_default());
    body
}

// =========================================================================
// Error mapping
// =========================================================================

fn map_err(e: TwitterClientError) -> ToolSourceError {
    match e {
        TwitterClientError::Request(err) => {
            ToolSourceError::Transport(format!("HTTP request failed: {err}"))
        }
        TwitterClientError::Api { code, msg } => {
            let hint = match code.as_str() {
                "401" => "API key 无效或已过期。请检查 TWITTER_API_KEY。",
                "403" => "访问被拒绝，可能是余额不足或权限不够。",
                "429" => "请求过于频繁，请稍后重试。",
                c if c.starts_with('5') => "服务器临时错误，可稍后重试。",
                _ => "",
            };
            if hint.is_empty() {
                ToolSourceError::Transport(format!("API error ({code}): {msg}"))
            } else {
                ToolSourceError::Transport(format!("{hint} ({code}): {msg}"))
            }
        }
    }
}

// =========================================================================
// Enum dispatch — one variant per API endpoint
// =========================================================================

/// Identifies which API endpoint a tool calls.
#[derive(Clone, Copy)]
enum TwitterOp {
    // Search & Tweets
    SearchTweets,
    GetUserRecentTweets,
    GetTweetsByIds,
    GetTweetReplies,
    GetTweetQuotations,
    GetTweetRetweeters,
    GetTweetThreadContext,
    GetArticleByTweetId,
    // User
    GetUserByUsername,
    GetUserAbout,
    GetUsersByIds,
    GetUserFollowers,
    GetUserFollowings,
    GetUserVerifiedFollowers,
    GetUserMentions,
    CheckFollowRelationship,
    SearchUsers,
    // Lists
    GetListFollowers,
    GetListMembers,
    // Trends & Spaces
    GetTrends,
    GetSpaceDetail,
    // Community
    GetCommunityInfo,
    GetCommunityMembers,
    GetCommunityModerators,
    GetCommunityTweets,
    SearchCommunityTweets,
    // Write: Login
    Login,
    // Write: Tweets
    CreateTweet,
    DeleteTweet,
    LikeTweet,
    UnlikeTweet,
    Retweet,
    // Write: Follow
    FollowUser,
    UnfollowUser,
    // Write: DM
    SendDm,
    // Write: Media
    UploadMedia,
    // Write: Profile
    UpdateProfile,
    // Write: Community
    CreateCommunity,
    DeleteCommunity,
    JoinCommunity,
    LeaveCommunity,
    // Filter Rules
    AddTweetFilterRule,
    UpdateTweetFilterRule,
    DeleteTweetFilterRule,
    ListTweetFilterRules,
    // User Monitor
    AddUserToMonitor,
    RemoveUserFromMonitor,
    ListMonitoredUsers,
    // Points
    GetPointsBalance,
}

// =========================================================================
// Tool struct
// =========================================================================

/// A single Twitter API tool.
pub struct TwitterTool {
    name: &'static str,
    spec: ToolSpec,
    op: TwitterOp,
    client: Arc<TwitterClient>,
}

impl TwitterTool {
    fn new(
        name: &'static str,
        description: &str,
        schema: Value,
        op: TwitterOp,
        client: Arc<TwitterClient>,
    ) -> Self {
        Self {
            name,
            spec: make_spec(name, description, schema),
            op,
            client,
        }
    }
}

fn make_spec(name: &str, description: &str, schema: Value) -> ToolSpec {
    use crate::{ToolOutputHint, ToolOutputStrategy};
    ToolSpec {
        name: name.to_string(),
        description: Some(description.to_string()),
        input_schema: schema,
        output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::FileRefWithExcerpt)),
    }
}

#[async_trait]
impl Tool for TwitterTool {
    fn name(&self) -> &str {
        self.name
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let result = dispatch(self.op, &args, &self.client).await;
        let json_str = result.map_err(map_err)?.to_string();
        Ok(ToolCallContent::text(json_str))
    }
}

/// Dispatch to the correct handler based on the operation.
async fn dispatch(
    op: TwitterOp,
    args: &Value,
    client: &TwitterClient,
) -> Result<Value, TwitterClientError> {
    match op {
        // Search & Tweets
        TwitterOp::SearchTweets => h_search_tweets(args, client).await,
        TwitterOp::GetUserRecentTweets => h_get_user_recent_tweets(args, client).await,
        TwitterOp::GetTweetsByIds => h_get_tweets_by_ids(args, client).await,
        TwitterOp::GetTweetReplies => h_get_tweet_replies(args, client).await,
        TwitterOp::GetTweetQuotations => h_get_tweet_quotations(args, client).await,
        TwitterOp::GetTweetRetweeters => h_get_tweet_retweeters(args, client).await,
        TwitterOp::GetTweetThreadContext => h_get_tweet_thread_context(args, client).await,
        TwitterOp::GetArticleByTweetId => h_get_article_by_tweet_id(args, client).await,
        // User
        TwitterOp::GetUserByUsername => h_get_user_by_username(args, client).await,
        TwitterOp::GetUserAbout => h_get_user_about(args, client).await,
        TwitterOp::GetUsersByIds => h_get_users_by_ids(args, client).await,
        TwitterOp::GetUserFollowers => h_get_user_followers(args, client).await,
        TwitterOp::GetUserFollowings => h_get_user_followings(args, client).await,
        TwitterOp::GetUserVerifiedFollowers => h_get_user_verified_followers(args, client).await,
        TwitterOp::GetUserMentions => h_get_user_mentions(args, client).await,
        TwitterOp::CheckFollowRelationship => h_check_follow_relationship(args, client).await,
        TwitterOp::SearchUsers => h_search_users(args, client).await,
        // Lists
        TwitterOp::GetListFollowers => h_get_list_followers(args, client).await,
        TwitterOp::GetListMembers => h_get_list_members(args, client).await,
        // Trends & Spaces
        TwitterOp::GetTrends => h_get_trends(args, client).await,
        TwitterOp::GetSpaceDetail => h_get_space_detail(args, client).await,
        // Community
        TwitterOp::GetCommunityInfo => h_get_community_info(args, client).await,
        TwitterOp::GetCommunityMembers => h_get_community_members(args, client).await,
        TwitterOp::GetCommunityModerators => h_get_community_moderators(args, client).await,
        TwitterOp::GetCommunityTweets => h_get_community_tweets(args, client).await,
        TwitterOp::SearchCommunityTweets => h_search_community_tweets(args, client).await,
        // Write: Login
        TwitterOp::Login => h_login(args, client).await,
        // Write: Tweets
        TwitterOp::CreateTweet => h_create_tweet(args, client).await,
        TwitterOp::DeleteTweet => h_delete_tweet(args, client).await,
        TwitterOp::LikeTweet => h_like_tweet(args, client).await,
        TwitterOp::UnlikeTweet => h_unlike_tweet(args, client).await,
        TwitterOp::Retweet => h_retweet(args, client).await,
        // Write: Follow
        TwitterOp::FollowUser => h_follow_user(args, client).await,
        TwitterOp::UnfollowUser => h_unfollow_user(args, client).await,
        // Write: DM
        TwitterOp::SendDm => h_send_dm(args, client).await,
        // Write: Media
        TwitterOp::UploadMedia => h_upload_media(args, client).await,
        // Write: Profile
        TwitterOp::UpdateProfile => h_update_profile(args, client).await,
        // Write: Community
        TwitterOp::CreateCommunity => h_create_community(args, client).await,
        TwitterOp::DeleteCommunity => h_delete_community(args, client).await,
        TwitterOp::JoinCommunity => h_join_community(args, client).await,
        TwitterOp::LeaveCommunity => h_leave_community(args, client).await,
        // Filter Rules
        TwitterOp::AddTweetFilterRule => h_add_tweet_filter_rule(args, client).await,
        TwitterOp::UpdateTweetFilterRule => h_update_tweet_filter_rule(args, client).await,
        TwitterOp::DeleteTweetFilterRule => h_delete_tweet_filter_rule(args, client).await,
        TwitterOp::ListTweetFilterRules => h_list_tweet_filter_rules(args, client).await,
        // User Monitor
        TwitterOp::AddUserToMonitor => h_add_user_to_monitor(args, client).await,
        TwitterOp::RemoveUserFromMonitor => h_remove_user_from_monitor(args, client).await,
        TwitterOp::ListMonitoredUsers => h_list_monitored_users(args, client).await,
        // Points
        TwitterOp::GetPointsBalance => h_get_points_balance(args, client).await,
    }
}

// =========================================================================
// Handler functions — one per endpoint
// =========================================================================

// --- Search & Tweets ---

async fn h_search_tweets(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let query = req_str(args, "query")?;
    let query_type = opt_str(args, "queryType").unwrap_or_else(|| "Latest".to_string());
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/tweet/advanced_search?query={}&queryType={}&cursor={}",
        percent_encode(&query),
        percent_encode(&query_type),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_user_recent_tweets(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/user/last_tweets?userName={}&cursor={}",
        percent_encode(&username),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_tweets_by_ids(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_ids = req_str(args, "tweet_ids")?;
    let path = format!("/twitter/tweets?tweet_ids={}", percent_encode(&tweet_ids));
    client.get(&path).await
}

async fn h_get_tweet_replies(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let sort = opt_str(args, "sort").unwrap_or_else(|| "Relevance".to_string());
    let query_type = match sort.to_lowercase().as_str() {
        "latest" => "Latest",
        "likes" => "Likes",
        _ => "Relevance",
    };
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/tweet/replies/v2?tweetId={}&queryType={}&cursor={}",
        percent_encode(&tweet_id),
        query_type,
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_tweet_quotations(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/tweet/quotes?tweetId={}&cursor={}",
        percent_encode(&tweet_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_tweet_retweeters(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/tweet/retweeters?tweetId={}&cursor={}",
        percent_encode(&tweet_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_tweet_thread_context(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/tweet/thread_context?tweetId={}&cursor={}",
        percent_encode(&tweet_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_article_by_tweet_id(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let path = format!("/twitter/article?tweet_id={}", percent_encode(&tweet_id));
    client.get(&path).await
}

// --- User ---

async fn h_get_user_by_username(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let path = format!("/twitter/user/info?userName={}", percent_encode(&username));
    client.get(&path).await
}

async fn h_get_user_about(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let path = format!("/twitter/user_about?userName={}", percent_encode(&username));
    client.get(&path).await
}

async fn h_get_users_by_ids(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let user_ids = req_str(args, "user_ids")?;
    let path = format!("/twitter/user/batch_info_by_ids?userIds={}", percent_encode(&user_ids));
    client.get(&path).await
}

async fn h_get_user_followers(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/user/followers?userName={}&cursor={}",
        percent_encode(&username),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_user_followings(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/user/followings?userName={}&cursor={}",
        percent_encode(&username),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_user_verified_followers(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    // Need user_id — fetch user info first
    let user = client
        .get(&format!("/twitter/user/info?userName={}", percent_encode(&username)))
        .await?;
    let user_id = user
        .get("data")
        .and_then(|v| v.get("id"))
        .or_else(|| user.get("id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| client_input_error(format!("user not found: {username}")))?;
    let path = format!(
        "/twitter/user/verifiedFollowers?user_id={}&cursor={}",
        percent_encode(user_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_user_mentions(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/user/mentions?userName={}&cursor={}",
        percent_encode(&username),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_check_follow_relationship(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username_a = req_str(args, "username_a")?;
    let username_b = req_str(args, "username_b")?;
    let path = format!(
        "/twitter/user/check_follow_relationship?source_user_name={}&target_user_name={}",
        percent_encode(&username_a),
        percent_encode(&username_b)
    );
    client.get(&path).await
}

async fn h_search_users(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let query = req_str(args, "query")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/user/search?query={}&cursor={}",
        percent_encode(&query),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

// --- Lists ---

async fn h_get_list_followers(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let list_id = req_str(args, "list_id")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/list/followers?list_id={}&cursor={}",
        percent_encode(&list_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_list_members(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let list_id = req_str(args, "list_id")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/list/members?list_id={}&cursor={}",
        percent_encode(&list_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

// --- Trends & Spaces ---

async fn h_get_trends(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let woeid = req_f64(args, "woeid")? as u64;
    let mut path = format!("/twitter/trends?woeid={}", woeid);
    if let Some(c) = opt_int(args, "count") {
        path.push_str(&format!("&count={}", c));
    }
    client.get(&path).await
}

async fn h_get_space_detail(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let space_id = req_str(args, "space_id")?;
    let path = format!("/twitter/spaces/detail?space_id={}", percent_encode(&space_id));
    client.get(&path).await
}

// --- Community ---

async fn h_get_community_info(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let community_id = req_str(args, "community_id")?;
    let path = format!("/twitter/community/info?community_id={}", percent_encode(&community_id));
    client.get(&path).await
}

async fn h_get_community_members(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let community_id = req_str(args, "community_id")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/community/members?community_id={}&cursor={}",
        percent_encode(&community_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_community_moderators(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let community_id = req_str(args, "community_id")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/community/moderators?community_id={}&cursor={}",
        percent_encode(&community_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_get_community_tweets(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let community_id = req_str(args, "community_id")?;
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/community/tweets?community_id={}&cursor={}",
        percent_encode(&community_id),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

async fn h_search_community_tweets(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let query = req_str(args, "query")?;
    let query_type = opt_str(args, "queryType").unwrap_or_else(|| "Latest".to_string());
    let cursor = opt_str(args, "cursor").unwrap_or_default();
    let path = format!(
        "/twitter/community/get_tweets_from_all_community?query={}&queryType={}&cursor={}",
        percent_encode(&query),
        percent_encode(&query_type),
        percent_encode(&cursor)
    );
    client.get(&path).await
}

// --- Write: Login ---

async fn h_login(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let password = req_str(args, "password")?;
    let mut body = json!({
        "user_name": username,
        "email": username,
        "password": password,
        "proxy": std::env::var("TWITTER_PROXY").unwrap_or_default(),
    });
    if let Some(code) = opt_str(args, "two_fa_code") {
        body["totp_secret"] = json!(code);
    }
    client.post("/twitter/user_login_v2", body).await
}

// --- Write: Tweets ---

async fn h_create_tweet(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let text = req_str(args, "text")?;
    let body = write_body(&cookie(args), json!({ "tweet_text": text }));
    client.post("/twitter/create_tweet_v2", body).await
}

async fn h_delete_tweet(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let body = write_body(&cookie(args), json!({ "tweet_id": tweet_id }));
    client.post("/twitter/delete_tweet_v2", body).await
}

async fn h_like_tweet(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let body = write_body(&cookie(args), json!({ "tweet_id": tweet_id }));
    client.post("/twitter/like_tweet_v2", body).await
}

async fn h_unlike_tweet(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let body = write_body(&cookie(args), json!({ "tweet_id": tweet_id }));
    client.post("/twitter/unlike_tweet_v2", body).await
}

async fn h_retweet(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tweet_id = req_str(args, "tweet_id")?;
    let body = write_body(&cookie(args), json!({ "tweet_id": tweet_id }));
    client.post("/twitter/retweet_tweet_v2", body).await
}

// --- Write: Follow ---

async fn h_follow_user(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let body = write_body(&cookie(args), json!({ "user_name": username }));
    client.post("/twitter/follow_user_v2", body).await
}

async fn h_unfollow_user(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let username = req_str(args, "username")?;
    let body = write_body(&cookie(args), json!({ "user_name": username }));
    client.post("/twitter/unfollow_user_v2", body).await
}

// --- Write: DM ---

async fn h_send_dm(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let recipient_user_id = req_str(args, "recipient_user_id")?;
    let text = req_str(args, "text")?;
    let body = write_body(&cookie(args), json!({ "recipient_user_id": recipient_user_id, "text": text }));
    client.post("/twitter/send_dm_v2", body).await
}

// --- Write: Media ---

async fn h_upload_media(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let media = req_str(args, "media")?;
    let body = write_body(&cookie(args), json!({ "media": media }));
    client.post("/twitter/upload_media_v2", body).await
}

// --- Write: Profile ---

async fn h_update_profile(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let mut body = json!({});
    if let Some(n) = opt_str(args, "name") {
        body["name"] = json!(n);
    }
    if let Some(d) = opt_str(args, "description") {
        body["description"] = json!(d);
    }
    let body = write_body(&cookie(args), body);
    client.post("/twitter/update_profile_v2", body).await
}

// --- Write: Community ---

async fn h_create_community(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let name = req_str(args, "name")?;
    let description = req_str(args, "description")?;
    let body = write_body(&cookie(args), json!({ "name": name, "description": description }));
    client.post("/twitter/create_community_v2", body).await
}

async fn h_delete_community(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let community_id = req_str(args, "community_id")?;
    let community_name = req_str(args, "community_name")?;
    let body = write_body(&cookie(args), json!({ "community_id": community_id, "community_name": community_name }));
    client.post("/twitter/delete_community_v2", body).await
}

async fn h_join_community(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let community_id = req_str(args, "community_id")?;
    let body = write_body(&cookie(args), json!({ "community_id": community_id }));
    client.post("/twitter/join_community_v2", body).await
}

async fn h_leave_community(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let community_id = req_str(args, "community_id")?;
    let body = write_body(&cookie(args), json!({ "community_id": community_id }));
    client.post("/twitter/leave_community_v2", body).await
}

// --- Filter Rules ---

async fn h_add_tweet_filter_rule(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let tag = req_str(args, "tag")?;
    let value = req_str(args, "value")?;
    let interval_seconds = req_f64(args, "interval_seconds")?;
    let body = json!({ "tag": tag, "value": value, "interval_seconds": interval_seconds });
    client.post("/oapi/tweet_filter/add_rule", body).await
}

async fn h_update_tweet_filter_rule(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let rule_id = req_str(args, "rule_id")?;
    let tag = req_str(args, "tag")?;
    let value = req_str(args, "value")?;
    let interval_seconds = req_f64(args, "interval_seconds")?;
    let mut body = json!({ "rule_id": rule_id, "tag": tag, "value": value, "interval_seconds": interval_seconds });
    if let Some(v) = opt_int(args, "is_effect") {
        body["is_effect"] = json!(v as i32);
    }
    client.post("/oapi/tweet_filter/update_rule", body).await
}

async fn h_delete_tweet_filter_rule(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let rule_id = req_str(args, "rule_id")?;
    let body = json!({ "rule_id": rule_id });
    client.delete("/oapi/tweet_filter/delete_rule", body).await
}

async fn h_list_tweet_filter_rules(_args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    client.get("/oapi/tweet_filter/get_rules").await
}

// --- User Monitor ---

async fn h_add_user_to_monitor(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let x_user_name = req_str(args, "x_user_name")?;
    let body = json!({ "x_user_name": x_user_name });
    client.post("/oapi/x_user_stream/add_user_to_monitor_tweet", body).await
}

async fn h_remove_user_from_monitor(args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    let id_for_user = req_str(args, "id_for_user")?;
    let body = json!({ "id_for_user": id_for_user });
    client.post("/oapi/x_user_stream/remove_user_to_monitor_tweet", body).await
}

async fn h_list_monitored_users(_args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    client.get("/oapi/x_user_stream/get_user_to_monitor_tweet").await
}

// --- Points ---

async fn h_get_points_balance(_args: &Value, client: &TwitterClient) -> Result<Value, TwitterClientError> {
    client.get("/api/points/balance").await
}

// =========================================================================
// URL percent-encoding helper
// =========================================================================

/// Simple URL percent-encoder for query values.
///
/// Encodes all non-alphanumeric characters except `-_.~`.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

// =========================================================================
// Tool catalog & registration
// =========================================================================

/// Tool metadata for registration.
struct ToolDef {
    name: &'static str,
    description: &'static str,
    schema: Value,
    op: TwitterOp,
}

/// Returns all Twitter tool definitions in a flat catalog.
fn all_tool_defs() -> Vec<ToolDef> {
    vec![
        // ===== Search & Tweets =====
        ToolDef {
            name: "twitter_search_tweets",
            description: "按高级搜索语法搜索推文。适用于：按关键词、用户、时间等条件查推文。query 语法可参考 from:username, since:date, #hashtag 等；每页最多 20 条。返回中包含 next_cursor、has_next_page；若 has_next_page 为 true，将 next_cursor 传入下次请求即可翻页。",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索 query（必填），支持 from:用户, since:日期, #标签 等语法" },
                    "queryType": { "type": "string", "description": "Latest 或 Top，默认 Latest", "enum": ["Latest", "Top"] },
                    "cursor": { "type": "string", "description": "分页游标，从上一次返回的 next_cursor 获取" }
                },
                "required": ["query"]
            }),
            op: TwitterOp::SearchTweets,
        },
        ToolDef {
            name: "twitter_get_user_recent_tweets",
            description: "获取指定用户最近发布的推文列表。适用于查看某人时间线。count 默认 20，最大建议 200；返回中有 next_cursor、has_next_page，用于翻页。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "用户名（必填）" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::GetUserRecentTweets,
        },
        ToolDef {
            name: "twitter_get_tweets_by_ids",
            description: "根据推文 ID 列表批量获取推文详情。适用于已知若干推文 ID 时拉取完整内容。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_ids": { "type": "string", "description": "推文 ID 列表（必填），逗号分隔" }
                },
                "required": ["tweet_ids"]
            }),
            op: TwitterOp::GetTweetsByIds,
        },
        ToolDef {
            name: "twitter_get_tweet_replies",
            description: "获取某条推文下的回复列表。sort 可选 Relevance / Latest / Likes；每页最多 20 条。返回中有 next_cursor、has_next_page，用于翻页。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID（必填）" },
                    "sort": { "type": "string", "description": "排序：Relevance / Latest / Likes", "enum": ["Relevance", "Latest", "Likes"] },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::GetTweetReplies,
        },
        ToolDef {
            name: "twitter_get_tweet_quotations",
            description: "获取引用该推文的其他推文列表。每页约 20 条；返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID（必填）" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::GetTweetQuotations,
        },
        ToolDef {
            name: "twitter_get_tweet_retweeters",
            description: "获取转推该推文的用户列表。每页约 100；返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID（必填）" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::GetTweetRetweeters,
        },
        ToolDef {
            name: "twitter_get_tweet_thread_context",
            description: "获取该推文所在对话线程的上下文（回复链）。有分页时用 next_cursor。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID（必填）" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::GetTweetThreadContext,
        },
        ToolDef {
            name: "twitter_get_article_by_tweet_id",
            description: "按推文 ID 拉取链接对应的文章正文（长文/链接解析）。计费较高（约 100 credits/篇），适用于需要解析推文中外链正文时。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID（必填）" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::GetArticleByTweetId,
        },
        // ===== User =====
        ToolDef {
            name: "twitter_get_user_by_username",
            description: "按用户名（screen name，即 @ 后面的名称）获取用户资料：昵称、简介、粉丝数、关注数等。当需要了解某个账号的基本信息时使用。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "用户名（screen name），不含 @" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::GetUserByUsername,
        },
        ToolDef {
            name: "twitter_get_user_about",
            description: "获取用户的简介（About）内容。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "用户名，不含 @" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::GetUserAbout,
        },
        ToolDef {
            name: "twitter_get_users_by_ids",
            description: "根据用户 ID 列表批量获取用户资料。适用于已知多个用户 ID 时拉取信息。",
            schema: json!({
                "type": "object",
                "properties": {
                    "user_ids": { "type": "string", "description": "用户 ID 列表，逗号分隔" }
                },
                "required": ["user_ids"]
            }),
            op: TwitterOp::GetUsersByIds,
        },
        ToolDef {
            name: "twitter_get_user_followers",
            description: "获取指定用户的粉丝列表。count 默认 200；返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "用户名，不含 @" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::GetUserFollowers,
        },
        ToolDef {
            name: "twitter_get_user_followings",
            description: "获取该用户关注的账号列表。每页约 200；返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "用户名，不含 @" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::GetUserFollowings,
        },
        ToolDef {
            name: "twitter_get_user_verified_followers",
            description: "获取该用户的已认证（蓝 V）粉丝列表。每页约 20；返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "用户名，不含 @" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::GetUserVerifiedFollowers,
        },
        ToolDef {
            name: "twitter_get_user_mentions",
            description: "获取提到（@）该用户的所有推文。每页约 20；返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "用户名，不含 @" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::GetUserMentions,
        },
        ToolDef {
            name: "twitter_check_follow_relationship",
            description: "检查 username_a 与 username_b 之间的关注关系（是否互关、谁关注谁）。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username_a": { "type": "string", "description": "用户名 A，不含 @" },
                    "username_b": { "type": "string", "description": "用户名 B，不含 @" }
                },
                "required": ["username_a", "username_b"]
            }),
            op: TwitterOp::CheckFollowRelationship,
        },
        ToolDef {
            name: "twitter_search_users",
            description: "按关键词搜索用户。返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索关键词" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["query"]
            }),
            op: TwitterOp::SearchUsers,
        },
        // ===== Lists =====
        ToolDef {
            name: "twitter_get_list_followers",
            description: "获取某 Twitter 列表的订阅者。list_id 为列表 ID。返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "list_id": { "type": "string", "description": "列表 ID" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["list_id"]
            }),
            op: TwitterOp::GetListFollowers,
        },
        ToolDef {
            name: "twitter_get_list_members",
            description: "获取某 Twitter 列表的成员。list_id 为列表 ID。返回有 next_cursor、has_next_page。",
            schema: json!({
                "type": "object",
                "properties": {
                    "list_id": { "type": "string", "description": "列表 ID" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["list_id"]
            }),
            op: TwitterOp::GetListMembers,
        },
        // ===== Trends & Spaces =====
        ToolDef {
            name: "twitter_get_trends",
            description: "按 woeid（Where On Earth ID）获取该地区的 Twitter 趋势。woeid 为必填，如 1 为全球。详见 getxagent.com。",
            schema: json!({
                "type": "object",
                "properties": {
                    "woeid": { "type": "integer", "description": "地区 ID，必填。如 1 为全球" },
                    "count": { "type": "integer", "description": "返回条数，可选，默认 30" }
                },
                "required": ["woeid"]
            }),
            op: TwitterOp::GetTrends,
        },
        ToolDef {
            name: "twitter_get_space_detail",
            description: "按 Space ID 获取 Twitter Space 的详情。",
            schema: json!({
                "type": "object",
                "properties": {
                    "space_id": { "type": "string", "description": "Space ID" }
                },
                "required": ["space_id"]
            }),
            op: TwitterOp::GetSpaceDetail,
        },
        // ===== Community =====
        ToolDef {
            name: "twitter_get_community_info",
            description: "按 community_id 获取社区信息。",
            schema: json!({
                "type": "object",
                "properties": {
                    "community_id": { "type": "string", "description": "社区 ID" }
                },
                "required": ["community_id"]
            }),
            op: TwitterOp::GetCommunityInfo,
        },
        ToolDef {
            name: "twitter_get_community_members",
            description: "获取社区成员，支持 cursor 分页。",
            schema: json!({
                "type": "object",
                "properties": {
                    "community_id": { "type": "string", "description": "社区 ID" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["community_id"]
            }),
            op: TwitterOp::GetCommunityMembers,
        },
        ToolDef {
            name: "twitter_get_community_moderators",
            description: "获取社区管理员，支持 cursor 分页。",
            schema: json!({
                "type": "object",
                "properties": {
                    "community_id": { "type": "string", "description": "社区 ID" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["community_id"]
            }),
            op: TwitterOp::GetCommunityModerators,
        },
        ToolDef {
            name: "twitter_get_community_tweets",
            description: "获取社区推文，支持 cursor 分页。",
            schema: json!({
                "type": "object",
                "properties": {
                    "community_id": { "type": "string", "description": "社区 ID" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["community_id"]
            }),
            op: TwitterOp::GetCommunityTweets,
        },
        ToolDef {
            name: "twitter_search_community_tweets",
            description: "在所有社区中按关键词搜索推文；queryType 支持 Latest/Top。",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索关键词" },
                    "queryType": { "type": "string", "description": "Latest 或 Top，默认 Latest" },
                    "cursor": { "type": "string", "description": "分页游标" }
                },
                "required": ["query"]
            }),
            op: TwitterOp::SearchCommunityTweets,
        },
        // ===== Write: Login =====
        ToolDef {
            name: "twitter_login",
            description: "登录 Twitter 账号，获取 login_cookie。写操作需要先登录。返回 JSON 中包含 login_cookies。设置 TWITTER_PROXY 可用于写操作/登录。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "用户名或邮箱" },
                    "password": { "type": "string", "description": "密码" },
                    "two_fa_code": { "type": "string", "description": "可选的双因素认证码（TOTP）" }
                },
                "required": ["username", "password"]
            }),
            op: TwitterOp::Login,
        },
        // ===== Write: Tweets =====
        ToolDef {
            name: "twitter_create_tweet",
            description: "发布新推文。需要 login_cookie（通过 twitter_login 获取）或设置 TWITTER_LOGIN_COOKIE。",
            schema: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "推文内容" },
                    "login_cookie": { "type": "string", "description": "登录 cookie（可选，默认从 TWITTER_LOGIN_COOKIE 环境变量获取）" }
                },
                "required": ["text"]
            }),
            op: TwitterOp::CreateTweet,
        },
        ToolDef {
            name: "twitter_delete_tweet",
            description: "删除推文。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::DeleteTweet,
        },
        ToolDef {
            name: "twitter_like_tweet",
            description: "点赞推文。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::LikeTweet,
        },
        ToolDef {
            name: "twitter_unlike_tweet",
            description: "取消点赞推文。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::UnlikeTweet,
        },
        ToolDef {
            name: "twitter_retweet",
            description: "转推。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tweet_id": { "type": "string", "description": "推文 ID" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["tweet_id"]
            }),
            op: TwitterOp::Retweet,
        },
        // ===== Write: Follow =====
        ToolDef {
            name: "twitter_follow_user",
            description: "关注用户。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "要关注的用户名" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::FollowUser,
        },
        ToolDef {
            name: "twitter_unfollow_user",
            description: "取消关注用户。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "要取消关注的用户名" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["username"]
            }),
            op: TwitterOp::UnfollowUser,
        },
        // ===== Write: DM =====
        ToolDef {
            name: "twitter_send_dm",
            description: "发送私信。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "recipient_user_id": { "type": "string", "description": "收件人用户 ID" },
                    "text": { "type": "string", "description": "私信内容" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["recipient_user_id", "text"]
            }),
            op: TwitterOp::SendDm,
        },
        // ===== Write: Media =====
        ToolDef {
            name: "twitter_upload_media",
            description: "上传媒体（图片等）。接受 base64 或 URL。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "media": { "type": "string", "description": "base64 编码或 URL" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["media"]
            }),
            op: TwitterOp::UploadMedia,
        },
        // ===== Write: Profile =====
        ToolDef {
            name: "twitter_update_profile",
            description: "更新个人资料（昵称、简介）。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "新的昵称（可选）" },
                    "description": { "type": "string", "description": "新的简介（可选）" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                }
            }),
            op: TwitterOp::UpdateProfile,
        },
        // ===== Write: Community =====
        ToolDef {
            name: "twitter_create_community",
            description: "创建社区。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "社区名称" },
                    "description": { "type": "string", "description": "社区描述" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["name", "description"]
            }),
            op: TwitterOp::CreateCommunity,
        },
        ToolDef {
            name: "twitter_delete_community",
            description: "删除社区。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "community_id": { "type": "string", "description": "社区 ID" },
                    "community_name": { "type": "string", "description": "社区名称" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["community_id", "community_name"]
            }),
            op: TwitterOp::DeleteCommunity,
        },
        ToolDef {
            name: "twitter_join_community",
            description: "加入社区。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "community_id": { "type": "string", "description": "社区 ID" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["community_id"]
            }),
            op: TwitterOp::JoinCommunity,
        },
        ToolDef {
            name: "twitter_leave_community",
            description: "退出社区。需要 login_cookie。",
            schema: json!({
                "type": "object",
                "properties": {
                    "community_id": { "type": "string", "description": "社区 ID" },
                    "login_cookie": { "type": "string", "description": "登录 cookie" }
                },
                "required": ["community_id"]
            }),
            op: TwitterOp::LeaveCommunity,
        },
        // ===== Filter Rules =====
        ToolDef {
            name: "twitter_add_tweet_filter_rule",
            description: "新增 Webhook/WebSocket 推文过滤规则。",
            schema: json!({
                "type": "object",
                "properties": {
                    "tag": { "type": "string", "description": "规则标签" },
                    "value": { "type": "string", "description": "规则值" },
                    "interval_seconds": { "type": "number", "description": "轮询间隔（秒）" }
                },
                "required": ["tag", "value", "interval_seconds"]
            }),
            op: TwitterOp::AddTweetFilterRule,
        },
        ToolDef {
            name: "twitter_update_tweet_filter_rule",
            description: "更新推文过滤规则。",
            schema: json!({
                "type": "object",
                "properties": {
                    "rule_id": { "type": "string", "description": "规则 ID" },
                    "tag": { "type": "string", "description": "规则标签" },
                    "value": { "type": "string", "description": "规则值" },
                    "interval_seconds": { "type": "number", "description": "轮询间隔（秒）" },
                    "is_effect": { "type": "integer", "description": "1=生效，0=不生效" }
                },
                "required": ["rule_id", "tag", "value", "interval_seconds"]
            }),
            op: TwitterOp::UpdateTweetFilterRule,
        },
        ToolDef {
            name: "twitter_delete_tweet_filter_rule",
            description: "删除推文过滤规则。",
            schema: json!({
                "type": "object",
                "properties": {
                    "rule_id": { "type": "string", "description": "规则 ID" }
                },
                "required": ["rule_id"]
            }),
            op: TwitterOp::DeleteTweetFilterRule,
        },
        ToolDef {
            name: "twitter_list_tweet_filter_rules",
            description: "获取当前账号下全部推文过滤规则。",
            schema: json!({
                "type": "object",
                "properties": {}
            }),
            op: TwitterOp::ListTweetFilterRules,
        },
        // ===== User Monitor =====
        ToolDef {
            name: "twitter_add_user_to_monitor",
            description: "添加要监控发推的用户（实时流）。",
            schema: json!({
                "type": "object",
                "properties": {
                    "x_user_name": { "type": "string", "description": "X 用户 handle（不带 @）" }
                },
                "required": ["x_user_name"]
            }),
            op: TwitterOp::AddUserToMonitor,
        },
        ToolDef {
            name: "twitter_remove_user_from_monitor",
            description: "移除监控用户。id_for_user 可通过 twitter_list_monitored_users 获得。",
            schema: json!({
                "type": "object",
                "properties": {
                    "id_for_user": { "type": "string", "description": "监控记录 ID" }
                },
                "required": ["id_for_user"]
            }),
            op: TwitterOp::RemoveUserFromMonitor,
        },
        ToolDef {
            name: "twitter_list_monitored_users",
            description: "获取当前所有被监控的用户。",
            schema: json!({
                "type": "object",
                "properties": {}
            }),
            op: TwitterOp::ListMonitoredUsers,
        },
        // ===== Points =====
        ToolDef {
            name: "twitter_get_points_balance",
            description: "查询当前账号的剩余点数余额。通过后端 HTTP API GET /api/points/balance 获取，需使用当前配置的 API Key 鉴权。返回 points_balance 与 updated_at。",
            schema: json!({
                "type": "object",
                "properties": {}
            }),
            op: TwitterOp::GetPointsBalance,
        },
    ]
}

/// Registers all Twitter API tools on the given registry.
///
/// All tools share a single `TwitterClient` (connection-pooled).
/// Requires `TWITTER_API_KEY` environment variable.
pub fn register_twitter_tools(registry: &ToolRegistryLocked) {
    let api_key = std::env::var("TWITTER_API_KEY").unwrap_or_default();
    let client = Arc::new(TwitterClient::new(api_key));
    for def in all_tool_defs() {
        let tool = TwitterTool::new(def.name, def.description, def.schema, def.op, Arc::clone(&client));
        registry.register_sync(Box::new(tool));
    }
}

/// Registers all Twitter API tools with an explicit API key.
pub fn register_twitter_tools_with_key(
    registry: &ToolRegistryLocked,
    api_key: impl Into<Arc<str>>,
) {
    let client = Arc::new(TwitterClient::new(api_key));
    for def in all_tool_defs() {
        let tool = TwitterTool::new(def.name, def.description, def.schema, def.op, Arc::clone(&client));
        registry.register_sync(Box::new(tool));
    }
}
