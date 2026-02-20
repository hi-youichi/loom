//! Exa search tools: websearch and codesearch via Exa API (full-featured).

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

const EXA_SEARCH_URL: &str = "https://api.exa.ai/search";
const NUM_RESULTS_MAX: u64 = 100;

fn exa_search_url() -> String {
    std::env::var("EXA_SEARCH_URL").unwrap_or_else(|_| EXA_SEARCH_URL.to_string())
}

/// Parameters for a single Exa search request (aligned with Exa API).
struct ExaSearchParams {
    query: String,
    num_results: u64,
    search_type: String,
    category: Option<String>,
    include_domains: Option<Vec<String>>,
    exclude_domains: Option<Vec<String>>,
    start_published_date: Option<String>,
    end_published_date: Option<String>,
    text_max_chars: u64,
    request_highlights: bool,
    highlights_max_chars: Option<u64>,
}

impl ExaSearchParams {
    fn build_body(&self) -> serde_json::Value {
        let mut contents = serde_json::map::Map::new();
        contents.insert(
            "text".to_string(),
            json!({ "maxCharacters": self.text_max_chars }),
        );
        if self.request_highlights {
            let mut hi = serde_json::map::Map::new();
            hi.insert("maxCharacters".to_string(), json!(self.highlights_max_chars.unwrap_or(2000)));
            contents.insert("highlights".to_string(), json!(hi));
        }

        let mut body = serde_json::json!({
            "query": self.query,
            "numResults": self.num_results.min(NUM_RESULTS_MAX),
            "type": self.search_type,
            "contents": contents,
        });

        let obj = body.as_object_mut().unwrap();
        if let Some(ref c) = self.category {
            obj.insert("category".to_string(), json!(c));
        }
        if let Some(ref d) = self.include_domains {
            if !d.is_empty() {
                obj.insert("includeDomains".to_string(), json!(d));
            }
        }
        if let Some(ref d) = self.exclude_domains {
            if !d.is_empty() {
                obj.insert("excludeDomains".to_string(), json!(d));
            }
        }
        if let Some(ref s) = self.start_published_date {
            obj.insert("startPublishedDate".to_string(), json!(s));
        }
        if let Some(ref e) = self.end_published_date {
            obj.insert("endPublishedDate".to_string(), json!(e));
        }

        body
    }
}

async fn exa_search_request(
    api_key: &str,
    params: ExaSearchParams,
) -> Result<serde_json::Value, ToolSourceError> {
    let body = params.build_body();
    let client = reqwest::Client::new();
    let res = client
        .post(exa_search_url())
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
    if !res.status().is_success() {
        let status = res.status();
        let err_body = res.text().await.unwrap_or_default();
        return Err(ToolSourceError::Transport(format!(
            "Exa API error {}: {}",
            status, err_body
        )));
    }
    let out: serde_json::Value = res
        .json()
        .await
        .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
    Ok(out)
}

fn format_results(value: &serde_json::Value, text_max_per_result: usize) -> String {
    let results: &[serde_json::Value] = value
        .get("results")
        .and_then(|r| r.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let mut s = String::new();
    for (i, r) in results.iter().enumerate() {
        let title = r.get("title").and_then(|t| t.as_str()).unwrap_or("(no title)");
        let url = r.get("url").and_then(|u| u.as_str()).unwrap_or("");
        s.push_str(&format!("[{}] {}\n  URL: {}\n", i + 1, title, url));

        // Prefer highlights (LLM-selected snippets) when present
        let highlights = r
            .get("highlights")
            .and_then(|h| h.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !highlights.is_empty() {
            for line in &highlights {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    s.push_str(&format!("  • {}\n", trimmed.replace('\n', " ")));
                }
            }
        } else if let Some(summary) = r.get("summary").and_then(|v| v.as_str()) {
            let summary = summary.trim();
            if !summary.is_empty() {
                let excerpt = if summary.len() > text_max_per_result {
                    format!("{}...", &summary[..text_max_per_result])
                } else {
                    summary.to_string()
                };
                s.push_str(&format!("  {}\n", excerpt.replace('\n', " ")));
            }
        }

        // Fallback or supplement: full text (truncated)
        let text = r.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if !text.is_empty() && highlights.is_empty() && r.get("summary").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
            let excerpt = if text.len() > text_max_per_result {
                format!("{}...", &text[..text_max_per_result])
            } else {
                text.to_string()
            };
            s.push_str(&format!("  {}\n", excerpt.replace('\n', " ")));
        } else if !text.is_empty() && text.len() <= text_max_per_result && highlights.is_empty() {
            s.push_str(&format!("  {}\n", text.replace('\n', " ")));
        }
        s.push('\n');
    }
    if s.is_empty() {
        s = "No results.".to_string();
    }
    s
}

fn parse_optional_string_array(args: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    let arr = args.get(key)?.as_array()?;
    let v: Vec<String> = arr
        .iter()
        .filter_map(|x| x.as_str().map(String::from))
        .collect();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Web search via Exa (real-time web search).
pub struct ExaWebsearchTool {
    api_key: String,
}

impl ExaWebsearchTool {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl Tool for ExaWebsearchTool {
    fn name(&self) -> &str {
        "websearch"
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: "websearch".to_string(),
            description: Some(
                "Search the web using Exa. Use for current events and up-to-date information. \
                 Supports category, date range, and domain filters. Today's date should be used when searching for recent information."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." },
                    "numResults": { "type": "integer", "description": "Max results (1-100, default 10).", "default": 10 },
                    "type": { "type": "string", "enum": ["auto", "neural", "fast", "deep", "instant"], "default": "auto" },
                    "category": { "type": "string", "enum": ["company", "research paper", "news", "tweet", "personal site", "financial report", "people"], "description": "Focus results by category." },
                    "includeDomains": { "type": "array", "items": { "type": "string" }, "description": "Only return results from these domains." },
                    "excludeDomains": { "type": "array", "items": { "type": "string" }, "description": "Exclude results from these domains." },
                    "startPublishedDate": { "type": "string", "description": "ISO 8601 date; only results published after this." },
                    "endPublishedDate": { "type": "string", "description": "ISO 8601 date; only results published before this." }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing query".to_string()))?
            .to_string();
        let num_results = args.get("numResults").and_then(|v| v.as_u64()).unwrap_or(10);
        let search_type = args
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();
        let category = args.get("category").and_then(|v| v.as_str()).map(String::from);
        let include_domains = parse_optional_string_array(&args, "includeDomains");
        let exclude_domains = parse_optional_string_array(&args, "excludeDomains");
        let start_published_date = args.get("startPublishedDate").and_then(|v| v.as_str()).map(String::from);
        let end_published_date = args.get("endPublishedDate").and_then(|v| v.as_str()).map(String::from);

        let params = ExaSearchParams {
            query,
            num_results,
            search_type,
            category,
            include_domains,
            exclude_domains,
            start_published_date,
            end_published_date,
            text_max_chars: 6000,
            request_highlights: true,
            highlights_max_chars: Some(2000),
        };
        let out = exa_search_request(&self.api_key, params).await?;
        Ok(ToolCallContent {
            text: format_results(&out, 1500),
        })
    }
}

/// Code/documentation search via Exa (programming-related queries).
pub struct ExaCodesearchTool {
    api_key: String,
}

impl ExaCodesearchTool {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl Tool for ExaCodesearchTool {
    fn name(&self) -> &str {
        "codesearch"
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: "codesearch".to_string(),
            description: Some(
                "Search for code examples, docs, and API references using Exa. \
                 Use for libraries, SDKs, frameworks, and programming patterns."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Programming-related query (e.g. 'React useState examples', 'Python pandas filter')." },
                    "numResults": { "type": "integer", "description": "Max results (1-100, default 10).", "default": 10 },
                    "includeDomains": { "type": "array", "items": { "type": "string" }, "description": "e.g. ['github.com', 'docs.rs']." }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing query".to_string()))?
            .to_string();
        let num_results = args.get("numResults").and_then(|v| v.as_u64()).unwrap_or(10);
        let include_domains = parse_optional_string_array(&args, "includeDomains");

        let params = ExaSearchParams {
            query,
            num_results,
            search_type: "auto".to_string(),
            category: None,
            include_domains,
            exclude_domains: None,
            start_published_date: None,
            end_published_date: None,
            text_max_chars: 10000,
            request_highlights: true,
            highlights_max_chars: Some(3000),
        };
        let out = exa_search_request(&self.api_key, params).await?;
        Ok(ToolCallContent {
            text: format_results(&out, 2000),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn read_http_body(stream: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = stream.read(&mut tmp).await.unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let header_end = pos + 4;
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = buf[header_end..].to_vec();
                while body.len() < content_length {
                    let m = stream.read(&mut tmp).await.unwrap();
                    if m == 0 {
                        break;
                    }
                    body.extend_from_slice(&tmp[..m]);
                }
                return String::from_utf8_lossy(&body[..content_length]).to_string();
            }
        }
        String::new()
    }

    async fn write_http_response(stream: &mut TcpStream, status: &str, body: &str) {
        let resp = format!(
            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            status,
            body.len(),
            body
        );
        stream.write_all(resp.as_bytes()).await.unwrap();
    }

    #[test]
    fn exa_search_params_build_body_applies_optional_fields_and_limits() {
        let params = ExaSearchParams {
            query: "rust".to_string(),
            num_results: 500,
            search_type: "auto".to_string(),
            category: Some("news".to_string()),
            include_domains: Some(vec!["example.com".to_string()]),
            exclude_domains: Some(vec!["spam.com".to_string()]),
            start_published_date: Some("2026-01-01".to_string()),
            end_published_date: Some("2026-02-01".to_string()),
            text_max_chars: 1234,
            request_highlights: true,
            highlights_max_chars: Some(456),
        };
        let body = params.build_body();
        assert_eq!(body["query"], "rust");
        assert_eq!(body["numResults"], NUM_RESULTS_MAX);
        assert_eq!(body["category"], "news");
        assert_eq!(body["includeDomains"][0], "example.com");
        assert_eq!(body["excludeDomains"][0], "spam.com");
        assert_eq!(body["startPublishedDate"], "2026-01-01");
        assert_eq!(body["endPublishedDate"], "2026-02-01");
        assert_eq!(body["contents"]["text"]["maxCharacters"], 1234);
        assert_eq!(body["contents"]["highlights"]["maxCharacters"], 456);
    }

    #[test]
    fn parse_optional_string_array_and_format_results_cover_fallbacks() {
        let args = json!({
            "includeDomains": ["a.com", 1, "b.com"],
            "excludeDomains": []
        });
        assert_eq!(
            parse_optional_string_array(&args, "includeDomains").unwrap(),
            vec!["a.com".to_string(), "b.com".to_string()]
        );
        assert!(parse_optional_string_array(&args, "excludeDomains").is_none());
        assert!(parse_optional_string_array(&args, "missing").is_none());

        let formatted = format_results(
            &json!({
                "results": [
                    {
                        "title": "T1",
                        "url": "https://a.com",
                        "highlights": ["line 1", "line 2"]
                    },
                    {
                        "title": "T2",
                        "url": "https://b.com",
                        "summary": "short summary"
                    },
                    {
                        "title": "T3",
                        "url": "https://c.com",
                        "text": "full text body"
                    }
                ]
            }),
            20,
        );
        assert!(formatted.contains("line 1"));
        assert!(formatted.contains("short summary"));
        assert!(formatted.contains("full text body"));
    }

    #[test]
    fn exa_tools_specs_and_missing_query_errors_are_valid() {
        let web = ExaWebsearchTool::new("k".to_string());
        let code = ExaCodesearchTool::new("k".to_string());
        assert_eq!(web.name(), "websearch");
        assert_eq!(code.name(), "codesearch");
        assert!(web.spec().description.unwrap_or_default().contains("Search the web"));
        assert!(code.spec().description.unwrap_or_default().contains("code"));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let web_err = rt
            .block_on(web.call(json!({}), None))
            .unwrap_err()
            .to_string();
        let code_err = rt
            .block_on(code.call(json!({}), None))
            .unwrap_err()
            .to_string();
        assert!(web_err.to_lowercase().contains("missing query"));
        assert!(code_err.to_lowercase().contains("missing query"));
    }

    #[tokio::test]
    async fn exa_requests_and_tools_use_overridden_url_for_success_and_error_paths() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let body = read_http_body(&mut stream).await;
                let req: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
                let query = req.get("query").and_then(|v| v.as_str()).unwrap_or("");
                match query {
                    "ok-web" => {
                        let out = json!({
                            "results":[{"title":"Web","url":"https://w","highlights":["h1"]}]
                        })
                        .to_string();
                        write_http_response(&mut stream, "200 OK", &out).await;
                    }
                    "ok-code" => {
                        let out = json!({
                            "results":[{"title":"Code","url":"https://c","summary":"sum"}]
                        })
                        .to_string();
                        write_http_response(&mut stream, "200 OK", &out).await;
                    }
                    "err" => {
                        write_http_response(
                            &mut stream,
                            "500 Internal Server Error",
                            r#"{"error":"boom"}"#,
                        )
                        .await;
                    }
                    other => panic!("unexpected query: {}", other),
                }
            }
        });

        let old = std::env::var("EXA_SEARCH_URL").ok();
        std::env::set_var("EXA_SEARCH_URL", format!("http://{}", addr));

        let ok_params = ExaSearchParams {
            query: "ok-web".to_string(),
            num_results: 1,
            search_type: "auto".to_string(),
            category: None,
            include_domains: None,
            exclude_domains: None,
            start_published_date: None,
            end_published_date: None,
            text_max_chars: 1000,
            request_highlights: true,
            highlights_max_chars: Some(100),
        };
        let ok = exa_search_request("k", ok_params).await.unwrap();
        assert_eq!(ok["results"][0]["title"], "Web");

        let web = ExaWebsearchTool::new("k".to_string());
        let web_out = web
            .call(json!({"query":"ok-web","numResults":1}), None)
            .await
            .unwrap();
        assert!(web_out.text.contains("Web"));

        let code = ExaCodesearchTool::new("k".to_string());
        let code_out = code
            .call(json!({"query":"ok-code","numResults":1}), None)
            .await
            .unwrap();
        assert!(code_out.text.contains("Code"));

        let err_params = ExaSearchParams {
            query: "err".to_string(),
            num_results: 1,
            search_type: "auto".to_string(),
            category: None,
            include_domains: None,
            exclude_domains: None,
            start_published_date: None,
            end_published_date: None,
            text_max_chars: 1000,
            request_highlights: false,
            highlights_max_chars: None,
        };
        let err = exa_search_request("k", err_params).await.unwrap_err();
        assert!(err.to_string().contains("Exa API error"));

        if let Some(v) = old {
            std::env::set_var("EXA_SEARCH_URL", v);
        } else {
            std::env::remove_var("EXA_SEARCH_URL");
        }
        server.await.unwrap();
    }
}
