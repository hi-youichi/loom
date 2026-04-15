//! Integration tests for context compression (prune + compact).
//!
//! Verifies that the compression subgraph (`build_graph`) actually prunes old tool results
//! and compacts conversation history via LLM summarization when thresholds are exceeded.

mod init_logging;

use std::collections::HashMap;
use std::sync::Arc;

use loom::{
    compress::{
        build_graph, compaction::PRUNE_PLACEHOLDER, CompactionConfig, CompressionGraphNode,
    },
    tools_condition, ActNode, LlmClient, Message, MockLlm, MockToolSource, ObserveNode, ReActState,
    StateGraph, ThinkNode, END, START,
};

fn make_state(messages: Vec<Message>) -> ReActState {
    ReActState {
        messages,
        ..Default::default()
    }
}

fn large_tool_result(name: &str, char_count: usize) -> Message {
    Message::user(format!(
        "Tool {} returned: {}",
        name,
        "x".repeat(char_count)
    ))
}

// ---------- Test 4: no compression ----------

#[tokio::test]
async fn no_compression_when_within_all_limits() {
    let config = CompactionConfig {
        prune: true,
        auto: true,
        max_context_tokens: 200_000,
        prune_keep_tokens: 100_000,
        prune_minimum: Some(0),
        ..Default::default()
    };
    let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(""));
    let graph = build_graph(config, llm, None).expect("compile");

    let msgs = vec![
        Message::user("hello"),
        Message::assistant("hi there"),
        Message::user("Tool bash returned: ok"),
        Message::user("what next?"),
    ];
    let state = make_state(msgs.clone());
    let out = graph.invoke(state, None).await.unwrap();

    assert_eq!(out.messages.len(), msgs.len());
    for (a, b) in out.messages.iter().zip(msgs.iter()) {
        match (a, b) {
            (Message::User(a), Message::User(b)) => assert_eq!(a, b),
            (Message::Assistant(a), Message::Assistant(b)) => assert_eq!(a, b),
            (Message::System(a), Message::System(b)) => assert_eq!(a, b),
            _ => panic!("message type mismatch"),
        }
    }
}

// ---------- Test 1: prune ----------

#[tokio::test]
async fn prune_replaces_old_tool_results_via_compression_graph() {
    // Each tool result: "Tool X returned: " (18 chars) + 400 chars = 418 chars ≈ 104 tokens.
    // 3 tool results ≈ 312 tokens total. prune_keep_tokens = 120 keeps only the newest (~104 tokens).
    let config = CompactionConfig {
        prune: true,
        prune_keep_tokens: 120,
        prune_minimum: Some(0),
        auto: false,
        ..Default::default()
    };
    let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(""));
    let graph = build_graph(config, llm, None).expect("compile");

    let msgs = vec![
        Message::user("What time is it?"),
        large_tool_result("bash", 400),
        large_tool_result("grep", 400),
        large_tool_result("read_file", 400),
    ];
    let state = make_state(msgs);
    let out = graph.invoke(state, None).await.unwrap();

    assert_eq!(out.messages.len(), 4);

    // First message (non-tool) unchanged
    assert!(
        matches!(&out.messages[0], Message::User(loom::UserContent::Text(s)) if s == "What time is it?")
    );

    // Oldest two tool results pruned (each ~104 tokens, cumulative exceeds 120 after the newest)
    assert!(
        matches!(&out.messages[1], Message::User(loom::UserContent::Text(s)) if s == PRUNE_PLACEHOLDER)
    );
    assert!(
        matches!(&out.messages[2], Message::User(loom::UserContent::Text(s)) if s == PRUNE_PLACEHOLDER)
    );

    // Most recent tool result preserved (first ~104 tokens within 120 budget)
    assert!(
        matches!(&out.messages[3], Message::User(loom::UserContent::Text(s)) if s.starts_with("Tool read_file returned:"))
    );
}

// ---------- Test 2: compact ----------

#[tokio::test]
async fn compact_summarizes_messages_on_overflow() {
    // 5 messages, each ~100 chars = ~500 chars total ≈ 125 tokens.
    // max_context_tokens = 100, reserve = 10 → 125 + 10 = 135 > 100 → overflow.
    // compact_keep_recent = 2 → keep last 2, summarize first 3.
    let config = CompactionConfig {
        auto: true,
        max_context_tokens: 100,
        reserve_tokens: 10,
        compact_keep_recent: 2,
        prune: false,
        ..Default::default()
    };
    let summary_text = "User asked about time, assistant checked clock";
    let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(summary_text));
    let graph = build_graph(config, llm, None).expect("compile");

    let msgs = vec![
        Message::user("x".repeat(100)),
        Message::assistant("y".repeat(100)),
        Message::user("z".repeat(100)),
        Message::assistant("w".repeat(100)),
        Message::user("v".repeat(100)),
    ];
    let state = make_state(msgs.clone());
    let out = graph.invoke(state, None).await.unwrap();

    // 1 summary + 2 kept recent = 3
    assert_eq!(out.messages.len(), 3);

    // First message is the summary
    let expected_summary = format!("[Summary of earlier conversation]: {}", summary_text);
    assert!(matches!(&out.messages[0], Message::System(s) if s == &expected_summary));

    // Last 2 messages preserved verbatim
    assert!(matches!(&out.messages[1], Message::Assistant(p) if p.content == "w".repeat(100)));
    assert!(
        matches!(&out.messages[2], Message::User(loom::UserContent::Text(s)) if *s == "v".repeat(100))
    );
}

// ---------- Test 3: prune then compact ----------

#[tokio::test]
async fn prune_then_compact_combined() {
    // After prune replaces tool results with short placeholders (~30 chars each ≈ 7 tokens),
    // the non-tool messages must still be large enough to overflow max_context_tokens.
    // 7 messages: user + 4 tool results + 2 large recent messages.
    // After prune: user(16 chars) + 4 placeholders(~120 chars) + 2 recent(600 chars each) = ~1336 chars ≈ 334 tokens.
    // max_context_tokens = 80, reserve = 10 → 334 + 10 = 344 > 80 → overflow → compact fires.
    let config = CompactionConfig {
        prune: true,
        prune_keep_tokens: 30,
        prune_minimum: Some(0),
        auto: true,
        max_context_tokens: 80,
        reserve_tokens: 10,
        compact_keep_recent: 2,
        ..Default::default()
    };
    let summary_text = "Conversation summary after prune and compact";
    let llm: Arc<dyn LlmClient> = Arc::new(MockLlm::with_no_tool_calls(summary_text));
    let graph = build_graph(config, llm, None).expect("compile");

    let msgs = vec![
        Message::user("initial question"),
        large_tool_result("bash", 400),
        large_tool_result("grep", 400),
        large_tool_result("read", 400),
        large_tool_result("write", 400),
        Message::assistant("a".repeat(600)),
        Message::user("b".repeat(600)),
    ];
    let input_count = msgs.len();
    let state = make_state(msgs);
    let out = graph.invoke(state, None).await.unwrap();

    // Should have 1 summary + 2 recent = 3 messages (much less than input 7)
    assert_eq!(out.messages.len(), 3);
    assert!(out.messages.len() < input_count);

    // First message is the summary
    assert!(
        matches!(&out.messages[0], Message::System(s) if s.contains("[Summary of earlier conversation]"))
    );
    assert!(matches!(&out.messages[0], Message::System(s) if s.contains(summary_text)));

    // Last 2 messages are the original recent messages
    assert!(matches!(&out.messages[1], Message::Assistant(p) if p.content == "a".repeat(600)));
    assert!(
        matches!(&out.messages[2], Message::User(loom::UserContent::Text(s)) if *s == "b".repeat(600))
    );
}

// ---------- Test 5: full ReAct loop with compression ----------

#[tokio::test]
async fn compression_in_full_react_loop() {
    let compress_llm: Arc<dyn LlmClient> =
        Arc::new(MockLlm::with_no_tool_calls("Summary of conversation"));
    let think_llm: Arc<dyn LlmClient> = Arc::new(MockLlm::first_tools_then_end());

    // After prune replaces tool results with placeholders, the remaining non-tool messages
    // (assistant messages with 200 chars each) must still overflow max_context_tokens.
    // Pre-fill: 1 user + 10*(assistant 200 chars + tool result 200 chars) = 21 messages.
    // After prune: tool results → placeholders (~30 chars). Remaining:
    //   1 user(16 chars) + 10 assistants(2000 chars) + 10 placeholders(~300 chars) = ~2316 chars ≈ 579 tokens.
    // max_context_tokens = 50, reserve = 10 → 579 + 10 >> 50 → overflow → compact fires.
    let config = CompactionConfig {
        auto: true,
        max_context_tokens: 50,
        reserve_tokens: 10,
        compact_keep_recent: 3,
        prune: true,
        prune_keep_tokens: 30,
        prune_minimum: Some(0),
        ..Default::default()
    };

    let compression_graph = build_graph(config, compress_llm, None).expect("compress graph");
    let compress_node = Arc::new(CompressionGraphNode::new(compression_graph));

    let think_path_map: HashMap<String, String> =
        [("tools".into(), "act".into()), (END.into(), END.into())]
            .into_iter()
            .collect();

    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node("think", Arc::new(ThinkNode::new(Arc::clone(&think_llm))))
        .add_node(
            "act",
            Arc::new(ActNode::new(Box::new(MockToolSource::get_time_example()))),
        )
        .add_node("observe", Arc::new(ObserveNode::with_loop()))
        .add_node("compress", compress_node)
        .add_edge(START, "think")
        .add_conditional_edges(
            "think",
            Arc::new(|s: &ReActState| tools_condition(s).as_str().to_string()),
            Some(think_path_map),
        )
        .add_edge("act", "observe")
        .add_edge("observe", "compress")
        .add_edge("compress", "think");

    let compiled = graph.compile().expect("valid graph");

    let mut history: Vec<Message> = vec![Message::user("What time is it?")];
    for i in 0..10 {
        history.push(Message::assistant("a".repeat(200)));
        history.push(Message::user(format!(
            "Tool tool_{} returned: {}",
            i,
            "x".repeat(200)
        )));
    }
    let initial_count = history.len();

    let state = make_state(history);
    let out = compiled.invoke(state, None).await.unwrap();

    // Compression should have reduced message count significantly
    assert!(
        out.messages.len() < initial_count,
        "expected compression to reduce messages: got {} from initial {}",
        out.messages.len(),
        initial_count
    );

    // Should contain a summary message from compaction
    let has_summary = out.messages.iter().any(
        |m| matches!(m, Message::System(s) if s.contains("[Summary of earlier conversation]")),
    );
    assert!(
        has_summary,
        "expected a summary System message after compaction"
    );

    // Flow completed normally
    assert!(out.tool_calls.is_empty());
    assert!(out.tool_results.is_empty());
}
