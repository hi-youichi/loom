use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use loom::SqliteSaver;
use loom::{
    Checkpointer, Embedder, JsonSerializer, LanceStore, LlmClient, LlmResponse, Message, Namespace,
    Next, Node, RunnableConfig, StateGraph, Store, StoreError, ToolCall as ReActToolCall,
    ToolCallContent, ToolResult, ToolSource, ToolSpec, END, START,
};

#[derive(Clone)]
struct MockEmbedder;

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, StoreError> {
        Ok(texts
            .iter()
            .map(|text| {
                let hash = text.chars().map(|c| c as u32).sum::<u32>();
                let mut vec = vec![0.0f32; 384];
                for (i, &byte) in hash.to_le_bytes().iter().enumerate() {
                    if i < 384 {
                        vec[i] = byte as f32 / 255.0;
                    }
                }
                vec
            })
            .collect())
    }

    fn dimension(&self) -> usize {
        384
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct MemoryReActState {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ReActToolCall>,
    pub tool_results: Vec<ToolResult>,
    #[serde(skip)]
    pub store: Option<Arc<dyn Store>>,
    pub namespace: Namespace,
}

struct MemoryMockLlm;

#[async_trait]
impl LlmClient for MemoryMockLlm {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, loom::error::AgentError> {
        let last_user_msg = messages
            .iter()
            .rev()
            .find(|m| matches!(m, Message::User(_)));

        let content = if let Some(Message::User(text)) = last_user_msg {
            let lower = text.to_lowercase();
            if lower.contains("remember") {
                extract_and_format_tool_call("save_memory", text)
            } else if lower.contains("what's my name") || lower.contains("what is my name") {
                extract_and_format_tool_call("retrieve_memory", "name")
            } else if lower.contains("what do you know") || lower.contains("tell me about myself") {
                extract_and_format_tool_call("list_memories", "")
            } else if lower.contains("hobbies")
                || lower.contains("interests")
                || lower.contains("hobby")
            {
                extract_and_format_tool_call("retrieve_memory", "hobbies")
            } else if lower.contains("preferences")
                || lower.contains("what do i like")
                || lower.contains("what are my")
            {
                extract_and_format_tool_call("list_memories", "")
            } else if lower.contains("love")
                || lower.contains("like")
                || lower.contains("favorite")
                || lower.contains("prefer")
            {
                extract_and_format_tool_call("save_memory", text)
            } else {
                format!("Based on our conversation, here's my response to: {}", text)
            }
        } else {
            "I'm ready to help! Tell me your name or preferences, and I'll remember them."
                .to_string()
        };

        let tool_calls = if content.contains("tool_call:") {
            vec![parse_tool_call(&content)]
        } else {
            vec![]
        };

        Ok(LlmResponse {
            content,
            tool_calls,
        })
    }
}

fn extract_and_format_tool_call(tool_name: &str, text: &str) -> String {
    match tool_name {
        "save_memory" => {
            let info = text.replace("remember", "").trim().to_string();
            let args = serde_json::json!({ "info": info });
            format!(
                "tool_call:{{\"name\":\"{}\",\"arguments\":{}}}",
                tool_name, args
            )
        }
        "retrieve_memory" => {
            let args = serde_json::json!({ "key": text });
            format!(
                "tool_call:{{\"name\":\"{}\",\"arguments\":{}}}",
                tool_name, args
            )
        }
        "list_memories" => {
            format!(
                "tool_call:{{\"name\":\"{}\",\"arguments\":{{}}}}",
                tool_name
            )
        }
        _ => format!(
            "tool_call:{{\"name\":\"{}\",\"arguments\":{{}}}}",
            tool_name
        ),
    }
}

fn parse_tool_call(text: &str) -> ReActToolCall {
    let json_str = text.trim_start_matches("tool_call:");
    let value: serde_json::Value = serde_json::from_str(json_str).unwrap_or_default();
    ReActToolCall {
        name: value["name"].as_str().unwrap_or("unknown").to_string(),
        arguments: value["arguments"].to_string(),
        id: None,
    }
}

struct MemoryToolSource {
    store: Arc<dyn Store>,
    namespace: Namespace,
}

impl MemoryToolSource {
    fn new(store: Arc<dyn Store>, namespace: Namespace) -> Self {
        Self { store, namespace }
    }
}

#[async_trait::async_trait]
impl ToolSource for MemoryToolSource {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, loom::tool_source::ToolSourceError> {
        Ok(vec![
            ToolSpec {
                name: "save_memory".to_string(),
                description: Some("Save information to long-term memory. Use when user says 'remember' or shares preferences.".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "info": {
                            "type": "string",
                            "description": "Information to remember (e.g., 'name is Alice', 'likes coffee')"
                        }
                    },
                    "required": ["info"]
                }),
            },
            ToolSpec {
                name: "retrieve_memory".to_string(),
                description: Some("Retrieve specific memory by key. Use for questions like 'what's my name'.".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "Key to retrieve (e.g., 'name', 'preferences')"
                        }
                    },
                    "required": ["key"]
                }),
            },
            ToolSpec {
                name: "list_memories".to_string(),
                description: Some("List all stored memories for the user. Use for 'what do you know about me'.".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                }),
            },
        ])
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolCallContent, loom::tool_source::ToolSourceError> {
        match name {
            "save_memory" => {
                let info = arguments["info"].as_str().unwrap_or("").to_string();
                let timestamp = chrono::Utc::now().to_rfc3339();
                let key = format!(
                    "memory_{}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                );
                let value = serde_json::json!({
                    "info": info,
                    "timestamp": timestamp
                });
                self.store
                    .put(&self.namespace, &key, &value)
                    .await
                    .map_err(|e| loom::tool_source::ToolSourceError::Transport(e.to_string()))?;
                Ok(ToolCallContent {
                    text: format!("Saved to memory: {}", info),
                })
            }
            "retrieve_memory" => {
                let key = arguments["key"].as_str().unwrap_or("");
                let hits = self
                    .store
                    .search(&self.namespace, Some(key), Some(5))
                    .await
                    .map_err(|e| loom::tool_source::ToolSourceError::Transport(e.to_string()))?;
                if hits.is_empty() {
                    Ok(ToolCallContent {
                        text: format!("No memories found for '{}'", key),
                    })
                } else {
                    let memories: Vec<String> = hits
                        .iter()
                        .map(|h| h.value["info"].as_str().unwrap_or("").to_string())
                        .collect();
                    Ok(ToolCallContent {
                        text: format!("Found memories: {}", memories.join(", ")),
                    })
                }
            }
            "list_memories" => {
                let keys =
                    self.store.list(&self.namespace).await.map_err(|e| {
                        loom::tool_source::ToolSourceError::Transport(e.to_string())
                    })?;
                let mut memories = Vec::new();
                for key in keys {
                    if let Some(value) =
                        self.store.get(&self.namespace, &key).await.map_err(|e| {
                            loom::tool_source::ToolSourceError::Transport(e.to_string())
                        })?
                    {
                        if let Some(info) = value["info"].as_str() {
                            memories.push(info.to_string());
                        }
                    }
                }
                if memories.is_empty() {
                    Ok(ToolCallContent {
                        text: "No memories stored yet. Tell me something to remember!".to_string(),
                    })
                } else {
                    Ok(ToolCallContent {
                        text: format!("I remember: {}", memories.join("; ")),
                    })
                }
            }
            _ => Err(loom::tool_source::ToolSourceError::NotFound(format!(
                "Unknown tool: {}",
                name
            ))),
        }
    }
}

struct MemoryThinkNode {
    llm: Box<dyn LlmClient>,
}

impl MemoryThinkNode {
    fn new(llm: Box<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

#[async_trait::async_trait]
impl Node<MemoryReActState> for MemoryThinkNode {
    fn id(&self) -> &str {
        "think"
    }

    async fn run(
        &self,
        state: MemoryReActState,
    ) -> Result<(MemoryReActState, Next), loom::error::AgentError> {
        let response = self.llm.invoke(&state.messages).await?;

        let mut messages = state.messages;
        messages.push(Message::Assistant(response.content));

        Ok((
            MemoryReActState {
                messages,
                tool_calls: response.tool_calls,
                tool_results: state.tool_results,
                store: state.store,
                namespace: state.namespace,
            },
            Next::Continue,
        ))
    }
}

struct MemoryActNode {
    tools: Box<dyn ToolSource>,
}

impl MemoryActNode {
    fn new(tools: Box<dyn ToolSource>) -> Self {
        Self { tools }
    }
}

#[async_trait::async_trait]
impl Node<MemoryReActState> for MemoryActNode {
    fn id(&self) -> &str {
        "act"
    }

    async fn run(
        &self,
        state: MemoryReActState,
    ) -> Result<(MemoryReActState, Next), loom::error::AgentError> {
        let mut tool_results = Vec::with_capacity(state.tool_calls.len());

        for tc in &state.tool_calls {
            let args: serde_json::Value = if tc.arguments.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}))
            };

            let content = self
                .tools
                .call_tool(&tc.name, args)
                .await
                .map_err(|e| loom::error::AgentError::ExecutionFailed(e.to_string()))?;

            tool_results.push(ToolResult {
                call_id: tc.id.clone(),
                name: Some(tc.name.clone()),
                content: content.text,
            });
        }

        Ok((
            MemoryReActState {
                messages: state.messages,
                tool_calls: state.tool_calls,
                tool_results,
                store: state.store,
                namespace: state.namespace,
            },
            Next::Continue,
        ))
    }
}

struct MemoryObserveNode {
    enable_loop: bool,
}

impl MemoryObserveNode {
    fn new() -> Self {
        Self { enable_loop: false }
    }
}

impl Default for MemoryObserveNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Node<MemoryReActState> for MemoryObserveNode {
    fn id(&self) -> &str {
        "observe"
    }

    async fn run(
        &self,
        state: MemoryReActState,
    ) -> Result<(MemoryReActState, Next), loom::error::AgentError> {
        let had_tool_calls = !state.tool_calls.is_empty();

        let mut messages = state.messages;
        for tr in &state.tool_results {
            let name = tr
                .name
                .as_deref()
                .or(tr.call_id.as_deref())
                .unwrap_or("tool");
            messages.push(Message::User(format!(
                "Tool {} returned: {}",
                name, tr.content
            )));
        }

        let next = if self.enable_loop && had_tool_calls {
            Next::Node("think".to_string())
        } else if self.enable_loop && !had_tool_calls {
            Next::End
        } else {
            Next::Continue
        };

        Ok((
            MemoryReActState {
                messages,
                tool_calls: vec![],
                tool_results: vec![],
                store: state.store,
                namespace: state.namespace,
            },
            next,
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== ReAct Agent with LanceDB Long-term Memory ===\n");

    let user_id = "demo_user";
    let namespace = vec![user_id.to_string(), "memories".to_string()];

    let lance_path = "lancedb_data";
    println!("Using LanceDB store at: {}", lance_path);
    let embedder = Arc::new(MockEmbedder);
    let store: Arc<dyn Store> = Arc::new(LanceStore::new(lance_path, embedder).await?);

    let serializer = Arc::new(JsonSerializer);

    let db_path = std::path::PathBuf::from("react_memory_demo.db");
    println!("Using SQLite checkpointer at: {}", db_path.display());

    let checkpointer: Arc<dyn Checkpointer<MemoryReActState>> =
        Arc::new(SqliteSaver::new(&db_path, serializer)?);

    let config = RunnableConfig {
        thread_id: Some(format!("session_{}", user_id)),
        checkpoint_id: None,
        checkpoint_ns: String::new(),
        user_id: Some(user_id.to_string()),
    };

    let tools = Box::new(MemoryToolSource::new(store.clone(), namespace.clone()));
    let llm = Box::new(MemoryMockLlm);

    let think = Arc::new(MemoryThinkNode::new(llm));
    let act = Arc::new(MemoryActNode::new(tools));
    let observe = Arc::new(MemoryObserveNode::new());

    let mut graph = StateGraph::<MemoryReActState>::new();
    graph
        .add_node("think", think)
        .add_node("act", act)
        .add_node("observe", observe)
        .add_edge(START, "think")
        .add_edge("think", "act")
        .add_edge("act", "observe")
        .add_edge("observe", END);

    let compiled = graph.compile_with_checkpointer(checkpointer.clone())?;

    let mut current_state = MemoryReActState {
        messages: vec![Message::system(
            "You are a helpful assistant with both short-term and long-term memory. \
             Use the save_memory tool when users share personal information. \
             Use retrieve_memory or list_memories when they ask about what you know about them.",
        )],
        tool_calls: vec![],
        tool_results: vec![],
        store: Some(store.clone()),
        namespace: namespace.clone(),
    };

    println!("Populating LanceDB with sample memories...\n");

    let sample_memories = [
        "name is Alice",
        "age is 28 years old",
        "lives in San Francisco",
        "works as a software engineer",
        "likes drinking coffee in morning",
        "favorite programming language is Rust",
        "enjoys hiking on weekends",
        "has a cat named Luna",
        "prefers tea over coffee in evening",
        "is learning about artificial intelligence",
        "birthday is on March 15",
        "graduated from Stanford University",
        "started working in tech 5 years ago",
        "loves reading science fiction",
        "plays guitar as a hobby",
        "enjoys Japanese cuisine",
        "favorite color is blue",
        "has been to 10 different countries",
        "loves outdoor adventures",
        "practices yoga twice a week",
        "collects vintage vinyl records",
    ];

    for memory in &sample_memories {
        let memory_key = format!(
            "memory_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let memory_value = serde_json::json!({
            "info": memory,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        store.put(&namespace, &memory_key, &memory_value).await?;
    }

    println!("✓ Stored {} memories in LanceDB\n", sample_memories.len());

    let test_queries = [
        "What's my name?",
        "What do you know about me?",
        "I love playing guitar",
        "hobby",
        "Remember I also enjoy swimming",
        "Tell me about my preferences",
    ];

    println!("Running demo queries...\n");

    for (idx, query) in test_queries.iter().enumerate() {
        println!("=== Query {}/{} ===", idx + 1, test_queries.len());
        println!("[User] {}", query);

        current_state
            .messages
            .push(Message::User(query.to_string()));

        match compiled
            .invoke(current_state.clone(), Some(config.clone()))
            .await
        {
            Ok(new_state) => {
                let new_messages: Vec<_> = new_state
                    .messages
                    .iter()
                    .skip(current_state.messages.len())
                    .collect();

                for msg in new_messages {
                    match msg {
                        Message::System(s) => println!("[System] {}", s),
                        Message::User(s) => println!("[User] {}", s),
                        Message::Assistant(s) => println!("[Assistant] {}", s),
                    }
                }

                println!(
                    "[Short-term Memory] {} messages in session",
                    new_state.messages.len()
                );
                let all_memories = store.list(&namespace).await?;
                println!("[Long-term Memory] {} memories stored", all_memories.len());

                current_state = new_state;
            }
            Err(e) => {
                eprintln!("\nError: {}", e);
                current_state.messages.pop();
            }
        }

        println!();
    }

    println!("=== Demo Summary ===");
    let checkpoint = checkpointer.get_tuple(&config).await?;
    if let Some((cp, _)) = checkpoint {
        println!("✓ Checkpoint saved: {}", cp.id);
        println!(
            "✓ Final state has {} messages",
            cp.channel_values.messages.len()
        );
    }
    let memories = store.list(&namespace).await?;
    println!("✓ Total memories stored: {}", memories.len());
    println!("✓ Memory types demonstrated:");
    println!("  - Short-term: Conversation history preserved across queries");
    println!("  - Long-term: 21 initial + dynamic additions stored in LanceDB with vector search");

    println!("\n=== Direct Long-term Memory Retrieval ===\n");

    println!("1. List all memory keys:");
    let keys = store.list(&namespace).await?;
    for (idx, key) in keys.iter().take(5).enumerate() {
        println!("   [{}] {}", idx + 1, key);
    }
    println!("   ... (showing first 5 of {} keys)\n", keys.len());

    println!("2. Retrieve a specific memory by key:");
    if let Some(first_key) = keys.first() {
        if let Some(value) = store.get(&namespace, first_key).await? {
            let info = value["info"].as_str().unwrap_or("N/A");
            let timestamp = value["timestamp"].as_str().unwrap_or("N/A");
            println!("   Key: {}", first_key);
            println!("   Info: {}", info);
            println!("   Timestamp: {}\n", timestamp);
        }
    }

    println!("3. Semantic search with scores (vector similarity):");
    let search_terms = ["name", "hobby", "coffee", "university"];
    for term in &search_terms {
        let hits = store.search(&namespace, Some(term), Some(3)).await?;
        println!("   Search '{}':", term);
        for (idx, hit) in hits.iter().enumerate() {
            let info = hit.value["info"].as_str().unwrap_or("N/A");
            let score_display = hit.score.map_or("N/A".to_string(), |s| format!("{:.4}", s));
            println!("     [{}] {} (score: {})", idx + 1, info, score_display);
        }
        println!();
    }

    println!("4. List all memories without search (using list for full enumeration):");
    let all_keys = store.list(&namespace).await?;
    println!(
        "   Showing first 10 memories (total: {} stored):",
        all_keys.len()
    );
    for (idx, key) in all_keys.iter().take(10).enumerate() {
        if let Some(value) = store.get(&namespace, key).await? {
            let info = value["info"].as_str().unwrap_or("N/A");
            println!("     [{}] {}", idx + 1, info);
        }
    }
    println!();

    println!("5. Search for recent additions:");
    let recent_hits = store.search(&namespace, Some("swimming"), Some(2)).await?;
    println!("   Search 'swimming':");
    for (idx, hit) in recent_hits.iter().enumerate() {
        let info = hit.value["info"].as_str().unwrap_or("N/A");
        let score_display = hit.score.map_or("N/A".to_string(), |s| format!("{:.4}", s));
        println!("     [{}] {} (score: {})", idx + 1, info, score_display);
    }

    Ok(())
}
