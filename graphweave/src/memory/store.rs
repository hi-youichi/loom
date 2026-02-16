//! Store trait and StoreError for cross-thread memory.
//!
//! Aligns with BaseStore pattern (namespace, put, get, list, search).
//!
//! ## Core Types
//!
//! - [`Store`]: The main trait for persistent key-value stores.
//! - [`Item`]: Stored key-value pairs with metadata (namespace, key, value, timestamps).
//! - [`SearchItem`]: Search result with optional relevance score.
//! - [`StoreOp`]: Operations for batch execution (Get, Put, Search, Delete, ListNamespaces).
//!
//! ## Example
//!
//! ```rust,ignore
//! use graphweave::memory::{Store, Namespace};
//!
//! // Single operations
//! store.put(&namespace, "key1", &json!({"data": "value"})).await?;
//! let item = store.get(&namespace, "key1").await?;
//!
//! // Batch operations
//! let results = store.batch(vec![
//!     StoreOp::Get { namespace: ns.clone(), key: "k1".into() },
//!     StoreOp::Put { namespace: ns.clone(), key: "k2".into(), value: json!({}) },
//! ]).await?;
//! ```

use async_trait::async_trait;
use std::time::SystemTime;

/// Namespace for Store: e.g. (user_id, "memories") or (user_id, "preferences").
///
/// Namespace tuple for store keys. Each element in the vector represents
/// one level in the hierarchy, allowing for nested categorization.
///
/// ## Example
///
/// ```rust
/// use graphweave::memory::Namespace;
///
/// let ns: Namespace = vec!["users".into(), "user123".into(), "memories".into()];
/// ```
pub type Namespace = Vec<String>;

/// Error for store operations.
///
/// Callers do not depend on underlying backend errors (e.g. rusqlite, lancedb).
/// Use `?` with `serde_json::Error` via `From` impl for serialization failures.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// JSON or namespace serialization/deserialization failed.
    #[error("serialization: {0}")]
    Serialization(String),

    /// Backend storage error (e.g. DB I/O). Message is opaque to avoid leaking backend types.
    #[error("storage: {0}")]
    Storage(String),

    /// Key not found in given namespace (optional; get may use `Ok(None)` instead).
    #[error("not found")]
    NotFound,

    /// Embedding generation error (e.g. OpenAI API error).
    #[error("embedding: {0}")]
    EmbeddingError(String),
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Serialization(e.to_string())
    }
}

/// Represents a stored item with metadata.
///
/// Item with metadata for store operations. Contains the stored value along with
/// key, namespace, and timestamps for creation and last update.
///
/// ## Interaction
///
/// - Returned by [`Store::get`] and [`Store::batch`] (for GetOp).
/// - Stored via [`Store::put`] and [`Store::batch`] (for PutOp).
#[derive(Debug, Clone)]
pub struct Item {
    /// The stored data as a JSON value. Keys are filterable.
    pub value: serde_json::Value,
    /// Unique identifier within the namespace.
    pub key: String,
    /// Hierarchical path defining the collection in which this item resides.
    pub namespace: Namespace,
    /// Timestamp of item creation.
    pub created_at: SystemTime,
    /// Timestamp of last update.
    pub updated_at: SystemTime,
}

impl Item {
    /// Creates a new Item with the current timestamp for both created_at and updated_at.
    pub fn new(namespace: Namespace, key: String, value: serde_json::Value) -> Self {
        let now = SystemTime::now();
        Self {
            value,
            key,
            namespace,
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates an Item with explicit timestamps (useful for restoration from storage).
    pub fn with_timestamps(
        namespace: Namespace,
        key: String,
        value: serde_json::Value,
        created_at: SystemTime,
        updated_at: SystemTime,
    ) -> Self {
        Self {
            value,
            key,
            namespace,
            created_at,
            updated_at,
        }
    }
}

/// Represents an item returned from a search operation with additional metadata.
///
/// Extends [`Item`] with an optional relevance/similarity score. For key-value
/// or string-filter search, `score` is `None`. For semantic/vector search,
/// `score` is the similarity (e.g., cosine or L2).
#[derive(Debug, Clone)]
pub struct SearchItem {
    /// The base item data.
    pub item: Item,
    /// Relevance/similarity score if from a ranked operation; `None` for non-ranked search.
    pub score: Option<f64>,
}

impl SearchItem {
    /// Creates a SearchItem from an Item without a score (non-ranked search).
    pub fn from_item(item: Item) -> Self {
        Self { item, score: None }
    }

    /// Creates a SearchItem from an Item with a relevance score.
    pub fn with_score(item: Item, score: f64) -> Self {
        Self {
            item,
            score: Some(score),
        }
    }
}

/// Filter operators for search operations.
///
/// Supports exact matches and comparison operators.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    /// Equal to (same as direct value comparison).
    Eq(serde_json::Value),
    /// Not equal to.
    Ne(serde_json::Value),
    /// Greater than.
    Gt(serde_json::Value),
    /// Greater than or equal to.
    Gte(serde_json::Value),
    /// Less than.
    Lt(serde_json::Value),
    /// Less than or equal to.
    Lte(serde_json::Value),
}

/// Options for search operations.
///
/// Used to configure [`Store::search`] behavior.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Natural language search query for semantic search capabilities.
    pub query: Option<String>,
    /// Key-value pairs for filtering results based on exact matches or comparison operators.
    pub filter: Option<std::collections::HashMap<String, FilterOp>>,
    /// Maximum number of items to return in the search results. Default: 10.
    pub limit: usize,
    /// Number of matching items to skip for pagination. Default: 0.
    pub offset: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchOptions {
    /// Creates default search options with limit=10 and offset=0.
    pub fn new() -> Self {
        Self {
            query: None,
            filter: None,
            limit: 10,
            offset: 0,
        }
    }

    /// Sets the query for semantic search.
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    /// Sets the limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Sets the offset for pagination.
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }
}

/// Match type for namespace filtering in list operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceMatchType {
    /// Match from the start of the namespace.
    Prefix,
    /// Match from the end of the namespace.
    Suffix,
}

/// Condition for matching namespaces in list operations.
#[derive(Debug, Clone)]
pub struct MatchCondition {
    /// Type of namespace matching to perform.
    pub match_type: NamespaceMatchType,
    /// Namespace path pattern (supports "*" wildcard).
    pub path: Vec<String>,
}

impl MatchCondition {
    /// Creates a prefix match condition.
    pub fn prefix(path: Vec<String>) -> Self {
        Self {
            match_type: NamespaceMatchType::Prefix,
            path,
        }
    }

    /// Creates a suffix match condition.
    pub fn suffix(path: Vec<String>) -> Self {
        Self {
            match_type: NamespaceMatchType::Suffix,
            path,
        }
    }
}

/// Options for listing namespaces.
#[derive(Debug, Clone, Default)]
pub struct ListNamespacesOptions {
    /// Optional conditions for filtering namespaces.
    pub match_conditions: Vec<MatchCondition>,
    /// Maximum depth of namespace hierarchy to return.
    pub max_depth: Option<usize>,
    /// Maximum number of namespaces to return. Default: 100.
    pub limit: usize,
    /// Number of namespaces to skip for pagination. Default: 0.
    pub offset: usize,
}

impl ListNamespacesOptions {
    /// Creates default options with limit=100 and offset=0.
    pub fn new() -> Self {
        Self {
            match_conditions: Vec::new(),
            max_depth: None,
            limit: 100,
            offset: 0,
        }
    }

    /// Adds a prefix match condition.
    pub fn with_prefix(mut self, prefix: Vec<String>) -> Self {
        self.match_conditions.push(MatchCondition::prefix(prefix));
        self
    }

    /// Adds a suffix match condition.
    pub fn with_suffix(mut self, suffix: Vec<String>) -> Self {
        self.match_conditions.push(MatchCondition::suffix(suffix));
        self
    }

    /// Sets the maximum depth.
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Sets the limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// Operations for batch execution.
///
/// Op types for store operations (GetOp, PutOp, SearchOp, ListNamespacesOp).
/// Used with [`Store::batch`] for executing multiple operations efficiently.
#[derive(Debug, Clone)]
pub enum StoreOp {
    /// Retrieve a specific item by namespace and key.
    Get { namespace: Namespace, key: String },
    /// Store or update an item. Set `value` to `None` to delete.
    Put {
        namespace: Namespace,
        key: String,
        value: Option<serde_json::Value>,
    },
    /// Search for items within a namespace prefix.
    Search {
        namespace_prefix: Namespace,
        options: SearchOptions,
    },
    /// List namespaces matching the given conditions.
    ListNamespaces { options: ListNamespacesOptions },
}

/// Result from a batch operation.
///
/// Each variant corresponds to the result of a specific [`StoreOp`].
#[derive(Debug, Clone)]
pub enum StoreOpResult {
    /// Result of a Get operation: the item if found, or None.
    Get(Option<Item>),
    /// Result of a Put operation: success indicator.
    Put,
    /// Result of a Search operation: list of matching items with optional scores.
    Search(Vec<SearchItem>),
    /// Result of a ListNamespaces operation: list of matching namespaces.
    ListNamespaces(Vec<Namespace>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_error_from_serde_json_error() {
        let invalid = "not valid json {{{";
        let err: StoreError = serde_json::from_str::<serde_json::Value>(invalid)
            .unwrap_err()
            .into();
        match &err {
            StoreError::Serialization(s) => assert!(s.contains("expected value") || s.len() > 0),
            _ => panic!("expected Serialization variant"),
        }
    }

    /// **Scenario**: Display of each StoreError variant contains expected keywords.
    #[test]
    fn store_error_display_each_variant() {
        let s = StoreError::Serialization("err".into()).to_string();
        assert!(s.to_lowercase().contains("serialization"), "{}", s);
        let s = StoreError::Storage("io".into()).to_string();
        assert!(s.to_lowercase().contains("storage"), "{}", s);
        let s = StoreError::NotFound.to_string();
        assert!(s.to_lowercase().contains("not found"), "{}", s);
        let s = StoreError::EmbeddingError("api".into()).to_string();
        assert!(s.to_lowercase().contains("embedding"), "{}", s);
    }

    /// **Scenario**: StoreSearchHit key/value/score can be constructed and accessed.
    #[test]
    fn store_search_hit_fields() {
        let hit = StoreSearchHit {
            key: "k1".into(),
            value: serde_json::json!({"v": 1}),
            score: Some(0.9),
        };
        assert_eq!(hit.key, "k1");
        assert_eq!(hit.value.get("v").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(hit.score, Some(0.9));

        let hit_no_score = StoreSearchHit {
            key: "k2".into(),
            value: serde_json::Value::Null,
            score: None,
        };
        assert_eq!(hit_no_score.key, "k2");
        assert!(hit_no_score.score.is_none());
    }

    /// **Scenario**: Item can be created with new() and timestamps are set.
    #[test]
    fn item_new_sets_timestamps() {
        let ns: Namespace = vec!["users".into(), "u1".into()];
        let item = Item::new(ns.clone(), "key1".into(), serde_json::json!({"data": 42}));

        assert_eq!(item.namespace, ns);
        assert_eq!(item.key, "key1");
        assert_eq!(item.value.get("data").and_then(|v| v.as_i64()), Some(42));
        // created_at and updated_at should be approximately equal (same instant)
        assert!(item.created_at <= item.updated_at);
    }

    /// **Scenario**: Item with_timestamps allows explicit timestamps.
    #[test]
    fn item_with_explicit_timestamps() {
        use std::time::Duration;

        let ns: Namespace = vec!["docs".into()];
        let created = SystemTime::UNIX_EPOCH + Duration::from_secs(1000000);
        let updated = SystemTime::UNIX_EPOCH + Duration::from_secs(2000000);

        let item = Item::with_timestamps(
            ns.clone(),
            "doc1".into(),
            serde_json::json!({"title": "Test"}),
            created,
            updated,
        );

        assert_eq!(item.namespace, ns);
        assert_eq!(item.key, "doc1");
        assert_eq!(item.created_at, created);
        assert_eq!(item.updated_at, updated);
    }

    /// **Scenario**: SearchItem can be created from Item without score.
    #[test]
    fn search_item_from_item_no_score() {
        let item = Item::new(vec!["ns".into()], "k".into(), serde_json::json!({"x": 1}));
        let search_item = SearchItem::from_item(item);

        assert_eq!(search_item.item.key, "k");
        assert!(search_item.score.is_none());
    }

    /// **Scenario**: SearchItem with_score includes relevance score.
    #[test]
    fn search_item_with_score() {
        let item = Item::new(vec!["ns".into()], "k".into(), serde_json::json!({"x": 1}));
        let search_item = SearchItem::with_score(item, 0.95);

        assert_eq!(search_item.item.key, "k");
        assert_eq!(search_item.score, Some(0.95));
    }

    /// **Scenario**: SearchOptions builder pattern works correctly.
    #[test]
    fn search_options_builder() {
        let opts = SearchOptions::new()
            .with_query("test query")
            .with_limit(20)
            .with_offset(5);

        assert_eq!(opts.query, Some("test query".into()));
        assert_eq!(opts.limit, 20);
        assert_eq!(opts.offset, 5);
    }

    /// **Scenario**: ListNamespacesOptions builder pattern works correctly.
    #[test]
    fn list_namespaces_options_builder() {
        let opts = ListNamespacesOptions::new()
            .with_prefix(vec!["users".into()])
            .with_suffix(vec!["v1".into()])
            .with_max_depth(3)
            .with_limit(50);

        assert_eq!(opts.match_conditions.len(), 2);
        assert_eq!(
            opts.match_conditions[0].match_type,
            NamespaceMatchType::Prefix
        );
        assert_eq!(opts.match_conditions[0].path, vec!["users"]);
        assert_eq!(
            opts.match_conditions[1].match_type,
            NamespaceMatchType::Suffix
        );
        assert_eq!(opts.match_conditions[1].path, vec!["v1"]);
        assert_eq!(opts.max_depth, Some(3));
        assert_eq!(opts.limit, 50);
    }

    /// **Scenario**: MatchCondition helper methods create correct types.
    #[test]
    fn match_condition_constructors() {
        let prefix = MatchCondition::prefix(vec!["a".into(), "b".into()]);
        assert_eq!(prefix.match_type, NamespaceMatchType::Prefix);
        assert_eq!(prefix.path, vec!["a", "b"]);

        let suffix = MatchCondition::suffix(vec!["x".into()]);
        assert_eq!(suffix.match_type, NamespaceMatchType::Suffix);
        assert_eq!(suffix.path, vec!["x"]);
    }

    /// **Scenario**: StoreOp variants can be constructed.
    #[test]
    fn store_op_variants() {
        let get_op = StoreOp::Get {
            namespace: vec!["ns".into()],
            key: "k1".into(),
        };
        let put_op = StoreOp::Put {
            namespace: vec!["ns".into()],
            key: "k2".into(),
            value: Some(serde_json::json!({"v": 1})),
        };
        let search_op = StoreOp::Search {
            namespace_prefix: vec!["ns".into()],
            options: SearchOptions::new(),
        };
        let list_op = StoreOp::ListNamespaces {
            options: ListNamespacesOptions::new(),
        };

        // Verify they can be pattern matched
        match get_op {
            StoreOp::Get { namespace, key } => {
                assert_eq!(namespace, vec!["ns"]);
                assert_eq!(key, "k1");
            }
            _ => panic!("expected Get"),
        }
        match put_op {
            StoreOp::Put { value, .. } => assert!(value.is_some()),
            _ => panic!("expected Put"),
        }
        match search_op {
            StoreOp::Search { options, .. } => assert_eq!(options.limit, 10),
            _ => panic!("expected Search"),
        }
        match list_op {
            StoreOp::ListNamespaces { options } => assert_eq!(options.limit, 100),
            _ => panic!("expected ListNamespaces"),
        }
    }

    /// **Scenario**: FilterOp variants can be created with values.
    #[test]
    fn filter_op_variants() {
        let eq = FilterOp::Eq(serde_json::json!("active"));
        let ne = FilterOp::Ne(serde_json::json!("deleted"));
        let gt = FilterOp::Gt(serde_json::json!(10));
        let gte = FilterOp::Gte(serde_json::json!(10));
        let lt = FilterOp::Lt(serde_json::json!(100));
        let lte = FilterOp::Lte(serde_json::json!(100));

        assert_eq!(eq, FilterOp::Eq(serde_json::json!("active")));
        assert_ne!(eq, ne);
        assert_ne!(gt, gte);
        assert_ne!(lt, lte);
    }
}

/// A single hit returned by [`Store::search_simple`] (legacy API).
///
/// For key-value or string-filter search (e.g. [`crate::memory::InMemoryStore`], [`crate::memory::SqliteStore`]),
/// `score` is `None`. For semantic/vector search (e.g. LanceStore), `score` is the similarity (e.g. cosine or L2).
#[derive(Debug, Clone)]
pub struct StoreSearchHit {
    /// The key of the matched entry within the namespace.
    pub key: String,
    /// The stored value (JSON).
    pub value: serde_json::Value,
    /// Similarity score when using vector search; `None` for string-filter-only stores.
    pub score: Option<f64>,
}

/// Long-term cross-session store: namespace-isolated key-value with optional search.
///
/// Used for user preferences, long-term memories, and retrievable facts. Not tied to a single
/// thread; use [`Namespace`] (e.g. `[user_id, "memories"]`) for multi-tenant isolation. Differs
/// from [`crate::memory::Checkpointer`], which is per-thread checkpoint state.
///
/// Base trait for store backends.
///
/// ## Core Operations
///
/// - **put**: Store or update an item by `(namespace, key)`.
/// - **get**: Retrieve an item by `(namespace, key)`, returns `None` if not found.
/// - **delete**: Remove an item by `(namespace, key)`.
/// - **search**: Search for items within a namespace prefix with optional query and filters.
/// - **list_namespaces**: List namespaces matching given conditions.
/// - **batch**: Execute multiple operations efficiently in a single call.
///
/// ## Example
///
/// ```rust,ignore
/// use graphweave::memory::{Store, Namespace, SearchOptions};
///
/// let ns: Namespace = vec!["user123".into(), "memories".into()];
///
/// // Store an item
/// store.put(&ns, "mem1", &json!({"content": "User prefers dark mode"})).await?;
///
/// // Retrieve the item
/// if let Some(item) = store.get_item(&ns, "mem1").await? {
///     println!("Found: {:?}", item.value);
/// }
///
/// // Search within namespace
/// let results = store.search(&ns, SearchOptions::new().with_limit(5)).await?;
/// ```
#[async_trait]
pub trait Store: Send + Sync {
    /// Stores `value` under `namespace` and `key`. Replaces any existing value for that key.
    ///
    /// Creates a new [`Item`] with current timestamp or updates an existing one.
    async fn put(
        &self,
        namespace: &Namespace,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), StoreError>;

    /// Returns the value for `(namespace, key)`, or `None` if not found.
    ///
    /// This is the simple API that returns only the value. Use [`get_item`] for full item metadata.
    async fn get(
        &self,
        namespace: &Namespace,
        key: &str,
    ) -> Result<Option<serde_json::Value>, StoreError>;

    /// Returns the full [`Item`] for `(namespace, key)`, or `None` if not found.
    ///
    /// Unlike [`get`], this returns the complete item with metadata (timestamps, namespace, key).
    async fn get_item(&self, namespace: &Namespace, key: &str) -> Result<Option<Item>, StoreError>;

    /// Deletes the item at `(namespace, key)`.
    ///
    /// Returns `Ok(())` even if the item does not exist (idempotent delete).
    async fn delete(&self, namespace: &Namespace, key: &str) -> Result<(), StoreError>;

    /// Returns all keys in the given namespace (order is implementation-defined).
    async fn list(&self, namespace: &Namespace) -> Result<Vec<String>, StoreError>;

    /// Searches within the namespace prefix with the given options.
    ///
    /// - If `options.query` is `None`, returns items up to `options.limit`.
    /// - If `options.query` is set, filters by string match or semantic similarity
    ///   (implementation-defined).
    /// - Results include optional relevance scores for ranked search.
    async fn search(
        &self,
        namespace_prefix: &Namespace,
        options: SearchOptions,
    ) -> Result<Vec<SearchItem>, StoreError>;

    /// Lists namespaces matching the given options.
    ///
    /// - Filter by prefix/suffix using `options.match_conditions`.
    /// - Limit depth using `options.max_depth`.
    /// - Paginate using `options.limit` and `options.offset`.
    async fn list_namespaces(
        &self,
        options: ListNamespacesOptions,
    ) -> Result<Vec<Namespace>, StoreError>;

    /// Executes multiple operations in a single batch.
    ///
    /// The order of results matches the order of input operations.
    /// More efficient than calling individual operations for bulk data manipulation.
    async fn batch(&self, ops: Vec<StoreOp>) -> Result<Vec<StoreOpResult>, StoreError>;

    // --- Legacy API for backward compatibility ---

    /// Searches within the namespace (legacy API).
    ///
    /// Use [`search`] for the full-featured API with [`SearchOptions`].
    async fn search_simple(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<StoreSearchHit>, StoreError> {
        let options = SearchOptions {
            query: query.map(String::from),
            filter: None,
            limit: limit.unwrap_or(10),
            offset: 0,
        };
        let results = self.search(namespace, options).await?;
        Ok(results
            .into_iter()
            .map(|si| StoreSearchHit {
                key: si.item.key,
                value: si.item.value,
                score: si.score,
            })
            .collect())
    }
}
