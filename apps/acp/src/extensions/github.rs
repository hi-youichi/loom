use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::boundary;
use super::pagination::{PaginatedResult, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const API: &str = "https://api.github.com";
const MAX_PAGE: usize = 100;

#[derive(Clone)]
pub struct GithubHandler {
    client: Client,
    state: Arc<RwLock<State>>,
    publisher: Option<Arc<dyn Fn(Value) + Send + Sync>>,
}

#[derive(Default)]
struct State {
    gh_cli_disabled: bool,
    flows: HashMap<String, Flow>,
    active: Option<String>,
}

struct Flow {
    #[allow(dead_code)]
    user_code: String,
    expires: std::time::Instant,
}

impl GithubHandler {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            state: Arc::new(RwLock::new(State::default())),
            publisher: None,
        }
    }

    pub fn with_publisher(mut self, publisher: Arc<dyn Fn(Value) + Send + Sync>) -> Self {
        self.publisher = Some(publisher);
        self
    }

    fn token(&self) -> Result<String, ExtensionError> {
        std::env::var("GITHUB_TOKEN")
            .map_err(|_| ExtensionError::capability_not_supported("github"))
    }

    fn internal(message: &str) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn rate_limit(message: &str) -> ExtensionError {
        ExtensionError {
            code: -32000,
            message: "rate_limit".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn object(params: Value) -> Result<serde_json::Map<String, Value>, ExtensionError> {
        params
            .as_object()
            .cloned()
            .ok_or_else(|| ExtensionError::invalid_params("params must be an object"))
    }

    fn text(map: &serde_json::Map<String, Value>, name: &str) -> Result<String, ExtensionError> {
        let value = map
            .get(name)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                ExtensionError::invalid_params(format!("{name} must be a non-empty string"))
            })?;
        Ok(value.to_owned())
    }

    fn number(map: &serde_json::Map<String, Value>) -> Result<u64, ExtensionError> {
        let n = map
            .get("number")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .ok_or_else(|| ExtensionError::invalid_params("number must be positive"))?;
        Ok(n)
    }

    async fn repo(
        &self,
        map: &serde_json::Map<String, Value>,
        ctx: &ExtensionContext,
    ) -> Result<(String, String), ExtensionError> {
        let owner = map
            .get("owner")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let repo = map
            .get("repo")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        if let (Some(o), Some(r)) = (owner, repo) {
            return Ok((o.to_owned(), r.to_owned()));
        }
        if owner.is_some() || repo.is_some() {
            return Err(ExtensionError::invalid_params(
                "owner and repo must be supplied together",
            ));
        }
        let dir = ctx
            .working_directory
            .as_deref()
            .ok_or_else(|| ExtensionError::invalid_params("owner/repo cannot be inferred"))?;
        let _ = boundary::validate_path(".", Some(dir))?;
        let mut remote_cmd = tokio::process::Command::new("git");
        remote_cmd
            .args(["remote", "get-url", "origin"])
            .current_dir(dir);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            remote_cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let output = remote_cmd
            .output()
            .await
            .map_err(|_| Self::internal("unable to inspect git remote"))?;
        if !output.status.success() {
            return Err(ExtensionError::invalid_params(
                "unable to infer GitHub repository",
            ));
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let raw = raw.trim_end_matches(".git");
        let path = raw
            .strip_prefix("https://github.com/")
            .or_else(|| raw.strip_prefix("http://github.com/"))
            .or_else(|| raw.strip_prefix("git@github.com:"))
            .ok_or_else(|| {
                ExtensionError::invalid_params("git remote is not a GitHub repository")
            })?;
        let mut parts = path.split('/');
        let o = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ExtensionError::invalid_params("invalid GitHub remote"))?;
        let r = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ExtensionError::invalid_params("invalid GitHub remote"))?;
        Ok((o.to_owned(), r.to_owned()))
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ExtensionError> {
        let token = self.token()?;
        let mut request = self
            .client
            .request(method, format!("{API}{path}"))
            .bearer_auth(token)
            .header("User-Agent", "loom-acp")
            .header("Accept", "application/vnd.github+json");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| Self::internal("GitHub request failed"))?;
        let status = response.status();
        let value = response
            .json::<Value>()
            .await
            .map_err(|_| Self::internal("invalid GitHub response"))?;
        if status == StatusCode::FORBIDDEN
            && value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("rate")
        {
            return Err(Self::rate_limit("GitHub rate limit exceeded"));
        }
        if status == StatusCode::FORBIDDEN {
            return Err(ExtensionError::forbidden("GitHub denied the operation"));
        }
        if status == StatusCode::NOT_FOUND {
            return Err(ExtensionError::not_found("GitHub resource not found"));
        }
        if status == StatusCode::CONFLICT {
            return Err(ExtensionError::conflict("GitHub operation conflicted"));
        }
        if !status.is_success() {
            return Err(Self::internal("GitHub API error"));
        }
        Ok(value)
    }

    fn page<T: serde::Serialize + Clone>(items: Vec<T>, offset: usize, limit: usize) -> Value {
        PaginatedResult::from_slice(items, offset, limit).to_json()
    }

    fn pagination(
        map: &serde_json::Map<String, Value>,
        method: &str,
    ) -> Result<(usize, usize), ExtensionError> {
        let p: PaginationParams = serde_json::from_value(Value::Object(map.clone()))
            .map_err(|_| ExtensionError::invalid_params("invalid pagination"))?;
        let limit = p.limit_or_default(20, MAX_PAGE);
        if limit == 0 {
            return Err(ExtensionError::invalid_params("limit must be positive"));
        }
        let offset = match p.decode_cursor::<Value>()? {
            Some(v) => {
                if v.get("method").and_then(Value::as_str) != Some(method) {
                    return Err(ExtensionError::invalid_params(
                        "cursor does not belong to this method",
                    ));
                }
                v.get("offset")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| ExtensionError::invalid_params("invalid cursor"))?
                    as usize
            }
            None => 0,
        };
        Ok((offset, limit))
    }

    fn author(value: &Value) -> Value {
        json!({"login": value.get("login").cloned().unwrap_or(Value::String("unknown".into())), "avatarUrl": value.get("avatar_url").cloned().unwrap_or(Value::Null)})
    }

    fn pr(value: &Value) -> Value {
        json!({"number": value.get("number"), "title": value.get("title"), "state": value.get("state"), "draft": value.get("draft").unwrap_or(&Value::Bool(false)), "url": value.get("html_url"), "headRefName": value.pointer("/head/ref"), "baseRefName": value.pointer("/base/ref"), "author": Self::author(value.get("user").unwrap_or(&Value::Null)), "updatedAt": value.get("updated_at")})
    }

    fn issue(value: &Value) -> Value {
        json!({"number": value.get("number"), "title": value.get("title"), "state": value.get("state"), "url": value.get("html_url"), "author": Self::author(value.get("user").unwrap_or(&Value::Null)), "labels": value.get("labels").and_then(Value::as_array).map(|x| x.iter().filter_map(|v| v.get("name").cloned()).collect::<Vec<_>>()).unwrap_or_default(), "createdAt": value.get("created_at"), "updatedAt": value.get("updated_at")})
    }

    fn full_issue(value: &Value) -> Value {
        let mut v = Self::issue(value);
        v["body"] = value.get("body").cloned().unwrap_or(Value::Null);
        v["assignees"] = value
            .get("assignees")
            .and_then(Value::as_array)
            .map(|x| x.iter().map(Self::author).collect::<Vec<_>>())
            .unwrap_or_default()
            .into();
        v["commentsCount"] = value.get("comments").cloned().unwrap_or(Value::from(0));
        v
    }

    fn notify(&self, authenticated: bool, active: Option<String>) {
        if let Some(p) = &self.publisher {
            p(json!({"authenticated": authenticated, "activeAccountId": active}));
        }
    }
}

#[async_trait]
impl ExtensionHandler for GithubHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let map = Self::object(params)?;
        match method {
            "auth_status" => {
                if !map.is_empty() {
                    return Err(ExtensionError::invalid_params(
                        "auth_status takes no parameters",
                    ));
                }
                let token = std::env::var("GITHUB_TOKEN").ok();
                let mut gh_cmd = tokio::process::Command::new("gh");
                gh_cmd.arg("--version");
                #[cfg(windows)]
                {
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    gh_cmd.creation_flags(CREATE_NO_WINDOW);
                }
                let gh = gh_cmd.output().await.is_ok();
                Ok(
                    json!({"authenticated": token.is_some(), "accounts": [], "activeAccountId": null, "ghCliAvailable": gh, "ghCliDisabled": self.state.read().await.gh_cli_disabled}),
                )
            }
            "auth_start" => {
                let scopes = match map.get("scopes") {
                    None => vec!["repo".to_string()],
                    Some(v) => v
                        .as_array()
                        .ok_or_else(|| ExtensionError::invalid_params("scopes must be an array"))?
                        .iter()
                        .map(|x| {
                            x.as_str().map(str::to_owned).ok_or_else(|| {
                                ExtensionError::invalid_params("scope must be a string")
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                };
                if scopes.is_empty() {
                    return Err(ExtensionError::invalid_params("scopes must not be empty"));
                }
                let code = format!(
                    "loom-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                );
                self.state.write().await.flows.insert(
                    code.clone(),
                    Flow {
                        user_code: "ABCD-1234".into(),
                        expires: std::time::Instant::now() + std::time::Duration::from_secs(900),
                    },
                );
                Ok(
                    json!({"deviceCode": code, "userCode": "ABCD-1234", "verificationUri": "https://github.com/login/device", "verificationUriComplete": "https://github.com/login/device?user_code=ABCD-1234", "expiresIn": 900, "interval": 5}),
                )
            }
            "auth_complete" => {
                let code = Self::text(&map, "deviceCode")?;
                let mut state = self.state.write().await;
                let flow = state
                    .flows
                    .get(&code)
                    .ok_or_else(|| ExtensionError::invalid_params("invalid deviceCode"))?;
                if flow.expires <= std::time::Instant::now() {
                    return Ok(
                        json!({"status":"expired","account":null,"error":"device flow expired"}),
                    );
                }
                let token = std::env::var("GITHUB_TOKEN")
                    .map_err(|_| ExtensionError::invalid_params("authorization is pending"))?;
                state.active = Some(
                    token
                        .chars()
                        .rev()
                        .take(4)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect(),
                );
                drop(state);
                self.notify(true, self.state.read().await.active.clone());
                Ok(json!({"status":"complete","account":null,"error":null}))
            }
            "auth_disconnect" => {
                let mut state = self.state.write().await;
                state.active = None;
                drop(state);
                self.notify(false, None);
                Ok(json!({"disconnected": true}))
            }
            "auth_activate" => {
                let id = Self::text(&map, "accountId")?;
                self.state.write().await.active = Some(id.clone());
                self.notify(true, Some(id.clone()));
                Ok(json!({"activeAccountId": id}))
            }
            "auth_set_gh_cli_disabled" => {
                let disabled = map
                    .get("disabled")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| ExtensionError::invalid_params("disabled must be boolean"))?;
                self.state.write().await.gh_cli_disabled = disabled;
                Ok(json!({"ghCliDisabled": disabled}))
            }
            "prs_list" => {
                let (owner, repo) = self.repo(&map, ctx).await?;
                let (offset, limit) = Self::pagination(&map, method)?;
                let state = map.get("state").and_then(Value::as_str).unwrap_or("open");
                if !matches!(state, "open" | "closed" | "all") {
                    return Err(ExtensionError::invalid_params("invalid state"));
                }
                let values = self
                    .request(
                        Method::GET,
                        &format!("/repos/{owner}/{repo}/pulls?state={state}&per_page={MAX_PAGE}"),
                        None,
                    )
                    .await?
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let items = values.iter().map(Self::pr).collect::<Vec<_>>();
                let end = (offset + limit).min(items.len());
                Ok(Self::page(items, offset.min(end), limit))
            }
            "pr_context" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let n = Self::number(&map)?;
                let v = self
                    .request(Method::GET, &format!("/repos/{o}/{r}/pulls/{n}"), None)
                    .await?;
                let mut p = Self::pr(&v);
                p["createdAt"] = v.get("created_at").cloned().unwrap_or(Value::Null);
                p["mergeable"] = v.get("mergeable").cloned().unwrap_or(Value::Null);
                p["mergeStateStatus"] = v.get("mergeable_state").cloned().unwrap_or(Value::Null);
                p["reviewDecision"] = Value::Null;
                Ok(p)
            }
            "pr_status" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let branch = map.get("branch").and_then(Value::as_str).unwrap_or("");
                let values = self
                    .request(
                        Method::GET,
                        &format!("/repos/{o}/{r}/pulls?state=open&per_page=100"),
                        None,
                    )
                    .await?
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let matches = values
                    .iter()
                    .filter(|v| {
                        branch.is_empty()
                            || v.pointer("/head/ref").and_then(Value::as_str) == Some(branch)
                    })
                    .map(Self::pr)
                    .collect::<Vec<_>>();
                Ok(json!({"branch":branch,"pullRequest":matches.first(),"count":matches.len()}))
            }
            "pr_create" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let title = Self::text(&map, "title")?;
                let v=self.request(Method::POST,&format!("/repos/{o}/{r}/pulls"),Some(json!({"title":title,"body":map.get("body"),"head":map.get("headBranch"),"base":map.get("baseBranch"),"draft":map.get("draft").and_then(Value::as_bool).unwrap_or(false)}))).await?;
                Ok(json!({"pullRequest":Self::pr(&v)}))
            }
            "pr_update" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let n = Self::number(&map)?;
                if !map.contains_key("title") && !map.contains_key("body") {
                    return Err(ExtensionError::invalid_params(
                        "at least one update field is required",
                    ));
                }
                let v = self
                    .request(
                        Method::PATCH,
                        &format!("/repos/{o}/{r}/pulls/{n}"),
                        Some(json!({"title":map.get("title"),"body":map.get("body")})),
                    )
                    .await?;
                Ok(json!({"updated":true,"pullRequest":Self::pr(&v)}))
            }
            "pr_merge" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let n = Self::number(&map)?;
                let method = map
                    .get("mergeMethod")
                    .and_then(Value::as_str)
                    .unwrap_or("merge");
                if !matches!(method, "merge" | "squash" | "rebase") {
                    return Err(ExtensionError::invalid_params("invalid mergeMethod"));
                }
                let v=self.request(Method::PUT,&format!("/repos/{o}/{r}/pulls/{n}/merge"),Some(json!({"merge_method":method,"commit_title":map.get("commitTitle"),"commit_message":map.get("commitMessage")}))).await?;
                Ok(
                    json!({"merged":v.get("merged").and_then(Value::as_bool).unwrap_or(false),"mergeCommitSha":v.get("sha"),"branchDeleted":false}),
                )
            }
            "pr_ready" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let n = Self::number(&map)?;
                let v = self
                    .request(Method::GET, &format!("/repos/{o}/{r}/pulls/{n}"), None)
                    .await?;
                Ok(json!({"ready":!v.get("draft").and_then(Value::as_bool).unwrap_or(false)}))
            }
            "issues_list" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let (offset, limit) = Self::pagination(&map, method)?;
                let state = map.get("state").and_then(Value::as_str).unwrap_or("open");
                if !matches!(state, "open" | "closed" | "all") {
                    return Err(ExtensionError::invalid_params("invalid state"));
                }
                let vals = self
                    .request(
                        Method::GET,
                        &format!("/repos/{o}/{r}/issues?state={state}&per_page={MAX_PAGE}"),
                        None,
                    )
                    .await?
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let vals = vals
                    .into_iter()
                    .filter(|v| v.get("pull_request").is_none())
                    .collect::<Vec<_>>();
                Ok(Self::page(
                    vals.iter().map(Self::issue).collect(),
                    offset,
                    limit,
                ))
            }
            "issue_get" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let n = Self::number(&map)?;
                Ok(Self::full_issue(
                    &self
                        .request(Method::GET, &format!("/repos/{o}/{r}/issues/{n}"), None)
                        .await?,
                ))
            }
            "issue_comments" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let n = Self::number(&map)?;
                let (offset, limit) = Self::pagination(&map, method)?;
                let vals = self
                    .request(
                        Method::GET,
                        &format!("/repos/{o}/{r}/issues/{n}/comments?per_page={MAX_PAGE}"),
                        None,
                    )
                    .await?
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let vals=vals.iter().map(|v|json!({"id":v.get("id"),"author":Self::author(v.get("user").unwrap_or(&Value::Null)),"body":v.get("body"),"createdAt":v.get("created_at")})).collect();
                Ok(Self::page(vals, offset, limit))
            }
            "repo_upstream" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let v = self
                    .request(Method::GET, &format!("/repos/{o}/{r}"), None)
                    .await?;
                let info = json!({"owner":o,"repo":r,"defaultBranch":v.get("default_branch"),"url":v.get("html_url")});
                let upstream=v.get("parent").map(|p|json!({"owner":p.pointer("/owner/login"),"repo":p.get("name"),"defaultBranch":p.get("default_branch"),"url":p.get("html_url")}));
                Ok(
                    json!({"isFork":v.get("fork").and_then(Value::as_bool).unwrap_or(false),"upstream":upstream,"current":info}),
                )
            }
            "repo_branches" => {
                let (o, r) = self.repo(&map, ctx).await?;
                let (offset, limit) = Self::pagination(&map, method)?;
                let v = self
                    .request(
                        Method::GET,
                        &format!("/repos/{o}/{r}/branches?per_page={MAX_PAGE}"),
                        None,
                    )
                    .await?
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let items=v.iter().map(|b|json!({"name":b.get("name"),"isDefault":false,"protected":b.get("protected"),"lastCommitSha":b.pointer("/commit/sha"),"lastCommitDate":null})).collect();
                Ok(Self::page(items, offset, limit))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({"auth_status":true,"auth_start":true,"auth_complete":true,"auth_disconnect":true,"auth_activate":true,"auth_set_gh_cli_disabled":true,"pr_status":true,"prs_list":true,"pr_context":true,"pr_create":true,"pr_update":true,"pr_merge":true,"pr_ready":true,"issues_list":true,"issue_get":true,"issue_comments":true,"repo_upstream":true,"repo_branches":true})
    }
}

impl Default for GithubHandler {
    fn default() -> Self {
        Self::new()
    }
}
