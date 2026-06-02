//! OpenAI-compatible chat completions client using plain `reqwest`.
//!
//! This client is kept minimal for now. The full OpenAI client implementation
//! is maintained in the loom crate for better compatibility.

use std::collections::HashMap;
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::message::{Message, AssistantToolCall};
use crate::error::AgentError;
use crate::tool::ToolCall;
use crate::traits::{LlmClient, LlmResponse, LlmUsage, LlmHeaders, ToolCallDelta, MessageChunk, ModelInfo};

/// OpenAI-compatible client (for Zhipu, Kimi, DeepSeek, etc.)
#[derive(Clone)]
pub struct ChatOpenAICompat {
    base_url: String,
    api_key: String,
    model: String,
    headers: LlmHeaders,
    client: reqwest::Client,
}

impl ChatOpenAICompat {
    /// Creates a new OpenAI-compatible client.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            headers: LlmHeaders::default(),
            client: reqwest::Client::new(),
        }
    }

    /// Sets custom HTTP headers.
    pub fn with_headers(mut self, headers: LlmHeaders) -> Self {
        self.headers = headers;
        self
    }

    fn build_url(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{}{}", base, path)
        } else {
            format!("{}/v1{}", base, path)
        }
    }

    fn build_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {}", self.api_key));
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        
        if let Some(ref thread_id) = self.headers.thread_id {
            headers.insert("X-Thread-Id".to_string(), thread_id.clone());
        }
        if let Some(ref trace_id) = self.headers.trace_id {
            headers.insert("X-Trace-Id".to_string(), trace_id.clone());
        }
        
        for (key, value) in &self.headers.custom_headers {
            headers.insert(key.clone(), value.clone());
        }
        
        headers
    }

    async fn do_request(&self, body: Value) -> Result<Value, AgentError> {
        let url = self.build_url("/chat/completions");
        let headers = self.build_headers();
        
        let mut request = self.client.post(&url);
        for (key, value) in headers {
            request = request.header(&key, &value);
        }
        
        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    AgentError::ExecutionFailed("LLM request timeout".into())
                } else if e.is_connect() {
                    AgentError::ExecutionFailed(format!("LLM connection error: {}", e))
                } else {
                    AgentError::ExecutionFailed(format!("LLM request failed: {}", e))
                }
            })?;
        
        let status = response.status().as_u16();
        let text = response.text().await.map_err(|e| AgentError::ExecutionFailed(format!("response read: {}", e)))?;
        
        if status >= 200 && status < 300 {
            serde_json::from_str(&text).map_err(|e| AgentError::ExecutionFailed(format!("response parse: {}", e)))
        } else {
            Err(AgentError::ExecutionFailed(format!("API error ({}): {}", status, text)))
        }
    }

    fn build_request_body(&self, messages: &[Message], stream: bool) -> Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages.iter().map(|m| self.message_to_json(m)).collect::<Vec<_>>(),
            "stream": stream,
        });
        
        if stream {
            body["stream_options"] = serde_json::json!({"include_usage": true});
        }
        
        body
    }

    fn message_to_json(&self, msg: &Message) -> Value {
        match msg {
            Message::System(content) => serde_json::json!({
                "role": "system",
                "content": content
            }),
            Message::User(content) => {
                let text = content.as_text();
                serde_json::json!({
                    "role": "user",
                    "content": text.as_ref()
                })
            }
            Message::Assistant(payload) => {
                let mut obj = serde_json::json!({
                    "role": "assistant",
                    "content": payload.content
                });
                
                if !payload.tool_calls.is_empty() {
                    let tool_calls: Vec<_> = payload.tool_calls.iter().map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments
                            }
                        })
                    }).collect();
                    obj["tool_calls"] = serde_json::json!(tool_calls);
                }
                
                if let Some(ref reasoning) = payload.reasoning_content {
                    obj["reasoning_content"] = serde_json::json!(reasoning);
                }
                
                obj
            }
            Message::Tool { tool_call_id, content } => {
                serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": match content {
                        crate::message::ToolCallContent::Text(s) => s.clone(),
                        _ => serde_json::to_string(content).unwrap_or_default(),
                    }
                })
            }
        }
    }

    fn parse_response(&self, json: &Value) -> Result<LlmResponse, AgentError> {
        let choice = json.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| AgentError::ExecutionFailed("No choices in response".to_string()))?;
        
        let message = choice.get("message")
            .ok_or_else(|| AgentError::ExecutionFailed("No message in choice".to_string()))?;
        
        let content = message.get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        
        let reasoning_content = message.get("reasoning_content")
            .and_then(|r| r.as_str())
            .map(String::from);
        
        let tool_calls: Vec<AssistantToolCall> = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|arr| {
                arr.iter().filter_map(|tc| {
                    let id = tc.get("id")?.as_str()?.to_string();
                    let function = tc.get("function")?;
                    let name = function.get("name")?.as_str()?.to_string();
                    let arguments = function.get("arguments")?.as_str()?.to_string();
                    
                    Some(AssistantToolCall { id, name, arguments })
                }).collect()
            })
            .unwrap_or_default();
        
        let tool_calls: Vec<ToolCall> = tool_calls.iter().map(ToolCall::from).collect();
        
        let usage = json.get("usage").map(|u| {
            LlmUsage {
                prompt_tokens: u.get("prompt_tokens").and_then(|p| p.as_u64()).unwrap_or(0) as u32,
                completion_tokens: u.get("completion_tokens").and_then(|c| c.as_u64()).unwrap_or(0) as u32,
                total_tokens: u.get("total_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as u32,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }
        });
        
        Ok(LlmResponse {
            content,
            reasoning_content,
            tool_calls,
            usage,
        })
    }
}

#[async_trait]
impl LlmClient for ChatOpenAICompat {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
        let body = self.build_request_body(messages, false);
        let json = self.do_request(body).await?;
        self.parse_response(&json)
    }

    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError> {
        self.invoke_stream_with_tool_delta(messages, chunk_tx, None).await
    }

    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
        _tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
    ) -> Result<LlmResponse, AgentError> {
        if chunk_tx.is_none() {
            return self.invoke(messages).await;
        }

        let body = self.build_request_body(messages, true);
        let url = self.build_url("/chat/completions");
        let headers = self.build_headers();
        
        let mut request = self.client.post(&url);
        for (key, value) in headers {
            request = request.header(&key, &value);
        }
        
        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::ExecutionFailed(format!("LLM connection error: {}", e)))?;
        
        let status = response.status().as_u16();
        if status != 200 {
            let text = response.text().await.unwrap_or_default();
            return Err(AgentError::ExecutionFailed(format!("API error ({}): {}", status, text)));
        }
        
        let stream = response.bytes_stream();
        let chunk_tx = chunk_tx.unwrap();
        
        use futures_util::StreamExt;
        let mut stream = stream.map(|r| r.map_err(|e| AgentError::ExecutionFailed(format!("stream error: {}", e))));
        
        let mut full_content = String::new();
        let mut reasoning_content: Option<String> = None;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage: Option<LlmUsage> = None;
        
        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            let text = String::from_utf8_lossy(&bytes);
            
            // Parse SSE lines
            for line in text.lines() {
                if !line.starts_with("data: ") {
                    continue;
                }
                
                let data = &line[6..];
                if data == "[DONE]" {
                    continue;
                }
                
                if let Ok(json) = serde_json::from_str::<Value>(data) {
                    // Process delta
                    if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                        for choice in choices {
                            if let Some(delta) = choice.get("delta") {
                                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                    full_content.push_str(content);
                                    let _ = chunk_tx.send(MessageChunk::message(content)).await;
                                }
                                
                                if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
                                    let rc = reasoning_content.get_or_insert_with(String::new);
                                    rc.push_str(reasoning);
                                    let _ = chunk_tx.send(MessageChunk::thinking(reasoning)).await;
                                }
                            }
                        }
                    }
                    
                    // Process usage at the end
                    if let Some(u) = json.get("usage") {
                        usage = Some(LlmUsage {
                            prompt_tokens: u.get("prompt_tokens").and_then(|p| p.as_u64()).unwrap_or(0) as u32,
                            completion_tokens: u.get("completion_tokens").and_then(|c| c.as_u64()).unwrap_or(0) as u32,
                            total_tokens: u.get("total_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as u32,
                            prompt_tokens_details: None,
                            completion_tokens_details: None,
                        });
                    }
                }
            }
        }
        
        Ok(LlmResponse {
            content: full_content,
            reasoning_content,
            tool_calls,
            usage,
        })
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, AgentError> {
        // Simple implementation - returns empty list
        Ok(Vec::new())
    }
}