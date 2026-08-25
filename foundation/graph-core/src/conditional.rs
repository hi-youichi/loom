//! Conditional edges: route to the next node based on state.
//!
//! Conditional edges: a source node has a
//! routing function that takes the current state and returns a key; the key is
//! either used as the next node id or looked up in an optional path map.
//!
//! **Interaction**: Used by `StateGraph::add_conditional_edges` and
//! `CompiledStateGraph` run loop to resolve the next node after a node with
//! conditional edges runs.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// Router function: takes a reference to state and returns a routing key.
///
/// The key is used as the next node id when no path map is provided, or
/// looked up in the path map to get the next node id (or END).
pub type ConditionalRouterFn<S> = Arc<dyn Fn(&S) -> String + Send + Sync>;

/// Entry in the next step map: either unconditional or conditional.
#[derive(Clone)]
pub enum NextEntry<S> {
    /// Unconditional edge to a specific node.
    Unconditional(String),
    /// Conditional edge with a router.
    Conditional(ConditionalRouter<S>),
}

/// Conditional edge definition: routing function plus optional path map.
///
/// - When `path_map` is `None`, the router's return value is used directly as the next node id.
/// - When `path_map` is `Some(map)`, the router's return value is used as the key;
///   the next node id is `map[key]` if present, otherwise the key itself (allowing
///   direct node ids as keys).
///
/// **Interaction**: Stored in `StateGraph` and `CompiledStateGraph`; invoked in the
/// run loop when the current node has conditional edges.
#[derive(Clone)]
pub struct ConditionalRouter<S> {
    /// Function that returns a routing key from the current state.
    pub(super) path: ConditionalRouterFn<S>,
    /// Optional map from routing key to node id (or END). If None, key is used as node id.
    pub(super) path_map: Option<HashMap<String, String>>,
}

impl<S> ConditionalRouter<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Builds a conditional router with an optional path map.
    pub fn new(path: ConditionalRouterFn<S>, path_map: Option<HashMap<String, String>>) -> Self {
        Self { path, path_map }
    }

    /// Resolves the next node id for a given state.
    ///
    /// Returns the resolved node id or "END" if routing terminates.
    pub fn resolve(&self, state: &S) -> String {
        let key = (self.path)(state);

        if let Some(map) = &self.path_map {
            map.get(&key).cloned().unwrap_or(key)
        } else {
            key
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_conditional_router_no_path_map() {
        let router: ConditionalRouter<String> =
            ConditionalRouter::new(Arc::new(|state| state.clone()), None);

        assert_eq!(router.resolve(&"node_a".to_string()), "node_a");
        assert_eq!(router.resolve(&"node_b".to_string()), "node_b");
    }

    #[test]
    fn test_conditional_router_with_path_map() {
        let mut path_map = HashMap::new();
        path_map.insert("approve".to_string(), "node_a".to_string());
        path_map.insert("reject".to_string(), "node_b".to_string());
        path_map.insert("review".to_string(), "END".to_string());

        let router: ConditionalRouter<String> =
            ConditionalRouter::new(Arc::new(|state| state.clone()), Some(path_map));

        assert_eq!(router.resolve(&"approve".to_string()), "node_a");
        assert_eq!(router.resolve(&"reject".to_string()), "node_b");
        assert_eq!(router.resolve(&"review".to_string()), "END");
    }

    #[test]
    fn test_conditional_router_unknown_key_falls_through() {
        let mut path_map = HashMap::new();
        path_map.insert("approve".to_string(), "node_a".to_string());

        let router: ConditionalRouter<String> =
            ConditionalRouter::new(Arc::new(|state| state.clone()), Some(path_map));

        // Unknown key falls through to use the key as the node id
        assert_eq!(router.resolve(&"unknown_node".to_string()), "unknown_node");
    }

    #[test]
    fn test_conditional_router_complex_state() {
        #[derive(Clone, Debug)]
        struct State {
            value: i32,
        }

        let mut path_map = HashMap::new();
        path_map.insert("low".to_string(), "node_a".to_string());
        path_map.insert("high".to_string(), "node_b".to_string());

        let router: ConditionalRouter<State> = ConditionalRouter::new(
            Arc::new(|state| {
                if state.value < 50 {
                    "low".to_string()
                } else {
                    "high".to_string()
                }
            }),
            Some(path_map),
        );

        assert_eq!(router.resolve(&State { value: 30 }), "node_a");
        assert_eq!(router.resolve(&State { value: 70 }), "node_b");
    }
}
