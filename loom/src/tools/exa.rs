//! Exa search tools: websearch and codesearch via Exa API (full-featured).

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

const EXA_SEARCH_URL: &str = "https://api.exa.ai/search";
const NUM_RESULTS_MAX: u64 = 100;

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
        .post(EXA_SEARCH_URL)
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
