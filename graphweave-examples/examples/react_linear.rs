//! ReAct linear chain: think → act → observe → END.
//!
//! Builds StateGraph<ReActState> with ThinkNode, ActNode, ObserveNode; one User message,
//! invoke once; MockLLM returns one get_time tool call, MockToolSource returns fixed time.
//!
//! Run: `cargo run -p graphweave-examples --example react_linear -- "What time is it?"`

use std::sync::Arc;

use graphweave::{
    ActNode, CompiledStateGraph, Message, MockLlm, MockToolSource, ObserveNode, ReActState,
    StateGraph, ThinkNode, END, REACT_SYSTEM_PROMPT, START,
};

#[tokio::main]
async fn main() {
    let input = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "What time is it?".to_string());

    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node(
            "think",
            Arc::new(ThinkNode::new(Arc::new(MockLlm::with_get_time_call()))),
        )
        .add_node(
            "act",
            Arc::new(ActNode::new(Box::new(MockToolSource::get_time_example()))),
        )
        .add_node("observe", Arc::new(ObserveNode::new()))
        .add_edge(START, "think")
        .add_edge("think", "act")
        .add_edge("act", "observe")
        .add_edge("observe", END);

    let compiled: CompiledStateGraph<ReActState> = graph.compile().expect("valid graph");

    let state = ReActState {
        messages: vec![Message::system(REACT_SYSTEM_PROMPT), Message::user(input)],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
    };

    match compiled.invoke(state, None).await {
        Ok(s) => {
            for m in &s.messages {
                match m {
                    Message::System(x) => println!("[System] {}", x),
                    Message::User(x) => println!("[User] {}", x),
                    Message::Assistant(x) => println!("[Assistant] {}", x),
                }
            }
            if s.messages.is_empty() {
                eprintln!("no messages");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}
