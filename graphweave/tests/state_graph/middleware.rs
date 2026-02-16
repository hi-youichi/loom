//! StateGraph middleware: compile_with_middleware and fluent with_middleware().compile().

use std::sync::Arc;

use async_trait::async_trait;
use graphweave::{AgentError, Message, Next, NodeMiddleware, StateGraph, END, START};

use crate::common::{AgentState, EchoAgent};

/// Logging middleware: records node ids as they run.
struct LoggingMiddleware {
    entered: std::sync::Mutex<Vec<String>>,
}

impl LoggingMiddleware {
    fn new() -> Self {
        Self {
            entered: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl NodeMiddleware<AgentState> for LoggingMiddleware {
    async fn around_run(
        &self,
        node_id: &str,
        state: AgentState,
        inner: Box<
            dyn FnOnce(
                    AgentState,
                ) -> std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = Result<(AgentState, Next), AgentError>>
                            + Send,
                    >,
                > + Send,
        >,
    ) -> Result<(AgentState, Next), AgentError> {
        self.entered.lock().unwrap().push(node_id.to_string());
        inner(state).await
    }
}

/// Compiled graph with `compile_with_middleware` wraps each node.run; invoke still produces correct output.
#[tokio::test]
async fn compile_with_middleware_wraps_node_run() {
    let middleware = Arc::new(LoggingMiddleware::new());
    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(EchoAgent::new()))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled = graph.compile_with_middleware(middleware.clone()).unwrap();
    let mut state = AgentState::default();
    state.messages.push(Message::User("hello".into()));

    let out = compiled.invoke(state, None).await.unwrap();
    assert!(matches!(out.messages.last(), Some(Message::Assistant(s)) if s == "hello"));

    let entered = middleware.entered.lock().unwrap();
    assert_eq!(entered.as_slice(), &["echo"]);
}

/// Fluent API: `with_middleware(m).compile()` wraps each node.run; invoke produces correct output.
#[tokio::test]
async fn with_middleware_compile_wraps_node_run() {
    let middleware = Arc::new(LoggingMiddleware::new());
    let mut graph = StateGraph::<AgentState>::new();
    graph
        .add_node("echo", Arc::new(EchoAgent::new()))
        .add_edge(START, "echo")
        .add_edge("echo", END);

    let compiled = graph.with_middleware(middleware.clone()).compile().unwrap();
    let mut state = AgentState::default();
    state.messages.push(Message::User("hello".into()));

    let out = compiled.invoke(state, None).await.unwrap();
    assert!(matches!(out.messages.last(), Some(Message::Assistant(s)) if s == "hello"));

    let entered = middleware.entered.lock().unwrap();
    assert_eq!(entered.as_slice(), &["echo"]);
}
