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
    ///
    /// - `path`: function `(state) -> key`. When `path_map` is None, `key` is the next node id.
    /// - `path_map`: if provided, `next_id = path_map.get(&key).unwrap_or(&key)`.
    pub fn new(path: ConditionalRouterFn<S>, path_map: Option<HashMap<String, String>>) -> Self {
        Self { path, path_map }
    }

    /// Resolves the next node id from the current state.
    ///
    /// Returns the node id (or END) to run next. Used by the compiled graph run loop.
    pub fn resolve_next(&self, state: &S) -> String {
        let key = (self.path)(state);
        self.path_map
            .as_ref()
            .and_then(|m| m.get(&key))
            .cloned()
            .unwrap_or(key)
    }
}

/// How to determine the next node after a given node runs.
///
/// Stored in the compiled graph's next map. For nodes with a single outgoing edge,
/// we use `Unconditional(to_id)`. For nodes with conditional edges, we use
/// `Conditional(router)` and resolve at runtime from state.
#[derive(Clone)]
pub enum NextEntry<S> {
    /// Single fixed next node (or END). Node's `Next` (Continue/Node/End) is still respected.
    Unconditional(String),
    /// Next node is decided by the router from state; the node's `Next` is ignored.
    Conditional(ConditionalRouter<S>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn resolve_without_path_map_uses_key_directly() {
        let router: ConditionalRouter<String> = ConditionalRouter::new(
            Arc::new(|s: &String| s.clone()),
            None,
        );
        assert_eq!(router.resolve_next(&"think".to_string()), "think");
        assert_eq!(router.resolve_next(&"act".to_string()), "act");
    }

    #[test]
    fn resolve_with_path_map_translates_key() {
        let mut map = HashMap::new();
        map.insert("yes".to_string(), "act".to_string());
        map.insert("no".to_string(), "think".to_string());
        let router: ConditionalRouter<String> = ConditionalRouter::new(
            Arc::new(|s: &String| s.clone()),
            Some(map),
        );
        assert_eq!(router.resolve_next(&"yes".to_string()), "act");
        assert_eq!(router.resolve_next(&"no".to_string()), "think");
    }

    #[test]
    fn resolve_with_path_map_unknown_key_passes_through() {
        let mut map = HashMap::new();
        map.insert("yes".to_string(), "act".to_string());
        let router: ConditionalRouter<String> = ConditionalRouter::new(
            Arc::new(|s: &String| s.clone()),
            Some(map),
        );
        // "maybe" is not in the map, so it's used directly
        assert_eq!(router.resolve_next(&"maybe".to_string()), "maybe");
    }

    #[test]
    fn resolve_with_empty_path_map_passes_through() {
        let router: ConditionalRouter<String> = ConditionalRouter::new(
            Arc::new(|s: &String| s.clone()),
            Some(HashMap::new()),
        );
        assert_eq!(router.resolve_next(&"anything".to_string()), "anything");
    }

    #[test]
    fn router_with_custom_routing_function() {
        let router: ConditionalRouter<i32> = ConditionalRouter::new(
            Arc::new(|n: &i32| if *n > 0 { "positive".to_string() } else { "negative".to_string() }),
            None,
        );
        assert_eq!(router.resolve_next(&5), "positive");
        assert_eq!(router.resolve_next(&-3), "negative");
        assert_eq!(router.resolve_next(&0), "negative");
    }
}
