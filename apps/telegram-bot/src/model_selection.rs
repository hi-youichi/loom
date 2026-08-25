use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use rusqlite::{params, Connection};

use crate::error::BotError;

use crate::constants::model::SEARCH_PAGE_SIZE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub model_id: String,
    pub display_name: String,
}

impl ModelChoice {
    pub fn new(model_id: impl Into<String>) -> Self {
        let model_id = model_id.into();
        Self {
            display_name: model_id.clone(),
            model_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSearchResult {
    pub query: String,
    pub page: usize,
    pub page_count: usize,
    pub items: Vec<ModelChoice>,
}

pub trait ModelSelectionStore: Send + Sync {
    fn get_selected_model(&self, chat_id: i64) -> Result<Option<String>, BotError>;
    fn save_selected_model(&self, chat_id: i64, model: &str) -> Result<(), BotError>;
    fn clear_selected_model(&self, chat_id: i64) -> Result<(), BotError>;
}

pub struct SqliteModelSelectionStore {
    database_path: std::path::PathBuf,
}

impl SqliteModelSelectionStore {
    pub fn new() -> Self {
        Self {
            database_path: checkpoint_sqlite_store::default_memory_db_path(),
        }
    }

    fn open_connection(&self) -> Result<Connection, BotError> {
        let connection = Connection::open(&self.database_path)?;
        connection
            .execute(
                "CREATE TABLE IF NOT EXISTS telegram_chat_model_selection (chat_id INTEGER PRIMARY KEY, model TEXT NOT NULL)",
                [],
            )
            ?;
        Ok(connection)
    }
}

impl Default for SqliteModelSelectionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelSelectionStore for SqliteModelSelectionStore {
    fn get_selected_model(&self, chat_id: i64) -> Result<Option<String>, BotError> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare("SELECT model FROM telegram_chat_model_selection WHERE chat_id = ?1")?;
        let mut rows = statement.query(params![chat_id])?;

        let row = rows.next()?;

        match row {
            Some(row) => Ok(row.get::<_, String>(0).map(Some)?),
            None => Ok(None),
        }
    }

    fn save_selected_model(&self, chat_id: i64, model: &str) -> Result<(), BotError> {
        let connection = self.open_connection()?;
        connection
            .execute(
                "INSERT INTO telegram_chat_model_selection (chat_id, model) VALUES (?1, ?2) ON CONFLICT(chat_id) DO UPDATE SET model = excluded.model",
                params![chat_id, model],
            )
            ?;
        Ok(())
    }

    fn clear_selected_model(&self, chat_id: i64) -> Result<(), BotError> {
        let connection = self.open_connection()?;
        connection.execute(
            "DELETE FROM telegram_chat_model_selection WHERE chat_id = ?1",
            params![chat_id],
        )?;
        Ok(())
    }
}

pub trait ModelCatalog: Send + Sync {
    fn default_model(&self) -> &str;
    fn search(&self, query: &str, page: usize) -> ModelSearchResult;
    fn has_model(&self, model_id: &str) -> bool;
}

pub struct StaticModelCatalog {
    default_model: String,
    models: Vec<ModelChoice>,
}

impl StaticModelCatalog {
    pub fn new(default_model: impl Into<String>, models: Vec<ModelChoice>) -> Self {
        let default_model = default_model.into();
        let mut unique_models = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for model in models
            .into_iter()
            .chain(std::iter::once(ModelChoice::new(default_model.clone())))
        {
            if seen.insert(model.model_id.clone()) {
                unique_models.push(model);
            }
        }

        unique_models.sort_by(|left, right| left.model_id.cmp(&right.model_id));

        Self {
            default_model,
            models: unique_models,
        }
    }
}

impl ModelCatalog for StaticModelCatalog {
    fn default_model(&self) -> &str {
        &self.default_model
    }

    fn search(&self, query: &str, page: usize) -> ModelSearchResult {
        let normalized_query = query.trim().to_lowercase();
        let filtered: Vec<ModelChoice> = if normalized_query.is_empty() {
            self.models.clone()
        } else {
            self.models
                .iter()
                .filter(|model| model.model_id.to_lowercase().contains(&normalized_query))
                .cloned()
                .collect()
        };

        let safe_page = page.max(1);
        let page_count = filtered.len().max(1).div_ceil(SEARCH_PAGE_SIZE);
        let bounded_page = safe_page.min(page_count);
        let start = (bounded_page - 1) * SEARCH_PAGE_SIZE;
        let end = (start + SEARCH_PAGE_SIZE).min(filtered.len());
        let items = if start < filtered.len() {
            filtered[start..end].to_vec()
        } else {
            Vec::new()
        };

        ModelSearchResult {
            query: query.trim().to_string(),
            page: bounded_page,
            page_count,
            items,
        }
    }

    fn has_model(&self, model_id: &str) -> bool {
        self.models.iter().any(|model| model.model_id == model_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSession {
    pub query: String,
    pub page: usize,
}

pub trait SearchSessionStore: Send + Sync {
    fn get_session(&self, chat_id: i64) -> Option<SearchSession>;
    fn save_session(&self, chat_id: i64, session: SearchSession);
    fn clear_session(&self, chat_id: i64);
}

#[derive(Default)]
pub struct InMemorySearchSessionStore {
    sessions: RwLock<HashMap<i64, SearchSession>>,
}

impl InMemorySearchSessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SearchSessionStore for InMemorySearchSessionStore {
    fn get_session(&self, chat_id: i64) -> Option<SearchSession> {
        self.sessions.read().ok()?.get(&chat_id).cloned()
    }

    fn save_session(&self, chat_id: i64, session: SearchSession) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.insert(chat_id, session);
        }
    }

    fn clear_session(&self, chat_id: i64) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.remove(&chat_id);
        }
    }
}

pub struct ModelSelectionService {
    catalog: Arc<dyn ModelCatalog>,
    store: Arc<dyn ModelSelectionStore>,
    sessions: Arc<dyn SearchSessionStore>,
}

impl ModelSelectionService {
    pub fn new(
        catalog: Arc<dyn ModelCatalog>,
        store: Arc<dyn ModelSelectionStore>,
        sessions: Arc<dyn SearchSessionStore>,
    ) -> Self {
        Self {
            catalog,
            store,
            sessions,
        }
    }

    pub fn current_model(&self, chat_id: i64) -> Result<String, BotError> {
        Ok(self
            .store
            .get_selected_model(chat_id)?
            .unwrap_or_else(|| self.catalog.default_model().to_string()))
    }

    pub fn search_models(&self, chat_id: i64, query: &str, page: usize) -> ModelSearchResult {
        let result = self.catalog.search(query, page);
        self.sessions.save_session(
            chat_id,
            SearchSession {
                query: result.query.clone(),
                page: result.page,
            },
        );
        result
    }

    pub fn next_page(&self, chat_id: i64) -> Option<ModelSearchResult> {
        let session = self.sessions.get_session(chat_id)?;
        Some(self.search_models(chat_id, &session.query, session.page + 1))
    }

    pub fn previous_page(&self, chat_id: i64) -> Option<ModelSearchResult> {
        let session = self.sessions.get_session(chat_id)?;
        let previous_page = session.page.saturating_sub(1).max(1);
        Some(self.search_models(chat_id, &session.query, previous_page))
    }

    pub fn select_model(&self, chat_id: i64, model_id: &str) -> Result<(), BotError> {
        if !self.catalog.has_model(model_id) {
            return Err(BotError::Config(format!("Unknown model: {model_id}")));
        }
        self.store.save_selected_model(chat_id, model_id)
    }

    pub fn clear_selection(&self, chat_id: i64) -> Result<(), BotError> {
        self.store.clear_selected_model(chat_id)?;
        self.sessions.clear_session(chat_id);
        Ok(())
    }

    pub fn clear_search_session(&self, chat_id: i64) {
        self.sessions.clear_session(chat_id);
    }
}

#[cfg(test)]
pub struct InMemoryModelSelectionStore {
    selected_models: RwLock<HashMap<i64, String>>,
}

#[cfg(test)]
impl InMemoryModelSelectionStore {
    pub fn new() -> Self {
        Self {
            selected_models: RwLock::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
impl ModelSelectionStore for InMemoryModelSelectionStore {
    fn get_selected_model(&self, chat_id: i64) -> Result<Option<String>, BotError> {
        Ok(self
            .selected_models
            .read()
            .map_err(|error| BotError::Unknown(error.to_string()))?
            .get(&chat_id)
            .cloned())
    }

    fn save_selected_model(&self, chat_id: i64, model: &str) -> Result<(), BotError> {
        self.selected_models
            .write()
            .map_err(|error| BotError::Unknown(error.to_string()))?
            .insert(chat_id, model.to_string());
        Ok(())
    }

    fn clear_selected_model(&self, chat_id: i64) -> Result<(), BotError> {
        self.selected_models
            .write()
            .map_err(|error| BotError::Unknown(error.to_string()))?
            .remove(&chat_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_returns_expected_page() {
        let catalog = StaticModelCatalog::new(
            "gpt-5.4",
            vec![
                ModelChoice::new("gpt-5.4"),
                ModelChoice::new("gpt-4.1"),
                ModelChoice::new("gpt-4o"),
            ],
        );

        let result = catalog.search("gpt-4", 1);

        assert_eq!(result.items.len(), 2);
        assert_eq!(result.page, 1);
        assert_eq!(result.page_count, 1);
    }

    #[test]
    fn service_uses_default_when_chat_has_no_override() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "gpt-5.4",
                vec![ModelChoice::new("gpt-5.4")],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );

        assert_eq!(service.current_model(1).unwrap(), "gpt-5.4");
    }

    // --- ModelChoice ---
    #[test]
    fn model_choice_new() {
        let choice = ModelChoice::new("gpt-4o");
        assert_eq!(choice.model_id, "gpt-4o");
        assert_eq!(choice.display_name, "gpt-4o");
    }

    #[test]
    fn model_choice_equality() {
        let a = ModelChoice::new("gpt-4o");
        let b = ModelChoice::new("gpt-4o");
        let c = ModelChoice::new("gpt-4.1");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // --- StaticModelCatalog ---
    #[test]
    fn catalog_deduplicates_and_sorts() {
        let catalog = StaticModelCatalog::new(
            "default",
            vec![
                ModelChoice::new("zeta"),
                ModelChoice::new("alpha"),
                ModelChoice::new("alpha"), // duplicate
            ],
        );
        assert_eq!(catalog.default_model(), "default");
        assert!(catalog.has_model("alpha"));
        assert!(catalog.has_model("zeta"));
        assert!(catalog.has_model("default"));
        assert!(!catalog.has_model("nonexistent"));

        let all = catalog.search("", 1);
        assert!(all.items.len() >= 3);
        // Should be sorted: alpha, default, zeta
        assert_eq!(all.items[0].model_id, "alpha");
    }

    #[test]
    fn catalog_search_empty_query_returns_all() {
        let catalog = StaticModelCatalog::new("default", vec![ModelChoice::new("m1")]);
        let result = catalog.search("", 1);
        assert!(!result.items.is_empty());
    }

    #[test]
    fn catalog_search_no_match() {
        let catalog = StaticModelCatalog::new("default", vec![ModelChoice::new("m1")]);
        let result = catalog.search("nonexistent", 1);
        assert!(result.items.is_empty());
        assert_eq!(result.page_count, 1);
    }

    #[test]
    fn catalog_search_page_zero_becomes_one() {
        let catalog = StaticModelCatalog::new("default", vec![ModelChoice::new("m1")]);
        let result = catalog.search("", 0);
        assert_eq!(result.page, 1);
    }

    // --- InMemorySearchSessionStore ---
    #[test]
    fn search_session_save_and_get() {
        let store = InMemorySearchSessionStore::new();
        assert!(store.get_session(1).is_none());
        store.save_session(
            1,
            SearchSession {
                query: "gpt".into(),
                page: 2,
            },
        );
        let session = store.get_session(1).unwrap();
        assert_eq!(session.query, "gpt");
        assert_eq!(session.page, 2);
    }

    #[test]
    fn search_session_clear() {
        let store = InMemorySearchSessionStore::new();
        store.save_session(
            1,
            SearchSession {
                query: "gpt".into(),
                page: 1,
            },
        );
        store.clear_session(1);
        assert!(store.get_session(1).is_none());
    }

    // --- InMemoryModelSelectionStore ---
    #[test]
    fn in_memory_store_save_get_clear() {
        let store = InMemoryModelSelectionStore::new();
        assert!(store.get_selected_model(1).unwrap().is_none());
        store.save_selected_model(1, "gpt-4o").unwrap();
        assert_eq!(store.get_selected_model(1).unwrap().unwrap(), "gpt-4o");
        store.clear_selected_model(1).unwrap();
        assert!(store.get_selected_model(1).unwrap().is_none());
    }

    // --- ModelSelectionService ---
    #[test]
    fn service_select_and_current_model() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "default",
                vec![ModelChoice::new("default"), ModelChoice::new("gpt-4o")],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );
        assert_eq!(service.current_model(1).unwrap(), "default");
        service.select_model(1, "gpt-4o").unwrap();
        assert_eq!(service.current_model(1).unwrap(), "gpt-4o");
    }

    #[test]
    fn service_select_unknown_model_errors() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "default",
                vec![ModelChoice::new("default")],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );
        let result = service.select_model(1, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn service_clear_selection() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "default",
                vec![ModelChoice::new("default")],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );
        service.select_model(1, "default").unwrap();
        service.clear_selection(1).unwrap();
        assert_eq!(service.current_model(1).unwrap(), "default");
    }

    #[test]
    fn service_search_and_pagination() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "default",
                vec![
                    ModelChoice::new("default"),
                    ModelChoice::new("gpt-4o"),
                    ModelChoice::new("gpt-4.1"),
                ],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );

        let result = service.search_models(1, "gpt", 1);
        assert!(!result.items.is_empty());

        // next_page should work after search
        let next = service.next_page(1);
        assert!(next.is_some());

        // previous_page should work
        let prev = service.previous_page(1);
        assert!(prev.is_some());
    }

    #[test]
    fn service_next_page_without_search_returns_none() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "default",
                vec![ModelChoice::new("default")],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );
        assert!(service.next_page(1).is_none());
    }

    #[test]
    fn service_previous_page_without_search_returns_none() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "default",
                vec![ModelChoice::new("default")],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );
        assert!(service.previous_page(1).is_none());
    }

    #[test]
    fn service_clear_search_session() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "default",
                vec![ModelChoice::new("default")],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );
        service.search_models(1, "test", 1);
        service.clear_search_session(1);
        assert!(service.next_page(1).is_none());
    }

    #[test]
    fn catalog_search_page_overflow() {
        let catalog = StaticModelCatalog::new("default", vec![ModelChoice::new("m1")]);
        let result = catalog.search("", 999);
        assert_eq!(result.page, 1); // bounded to page_count
    }

    #[test]
    fn model_search_result_equality() {
        let a = ModelSearchResult {
            query: "gpt".into(),
            page: 1,
            page_count: 1,
            items: vec![ModelChoice::new("gpt-4o")],
        };
        let b = ModelSearchResult {
            query: "gpt".into(),
            page: 1,
            page_count: 1,
            items: vec![ModelChoice::new("gpt-4o")],
        };
        assert_eq!(a, b);
    }

    #[test]
    fn search_session_equality() {
        let a = SearchSession {
            query: "test".into(),
            page: 1,
        };
        let b = SearchSession {
            query: "test".into(),
            page: 1,
        };
        let c = SearchSession {
            query: "test".into(),
            page: 2,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn previous_page_at_page_one_stays_at_one() {
        let service = ModelSelectionService::new(
            Arc::new(StaticModelCatalog::new(
                "default",
                vec![ModelChoice::new("default")],
            )),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );
        service.search_models(1, "default", 1);
        let prev = service.previous_page(1).unwrap();
        assert_eq!(prev.page, 1);
    }
}
