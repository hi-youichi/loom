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
            database_path: loom::memory::default_memory_db_path(),
        }
    }

    fn open_connection(&self) -> Result<Connection, BotError> {
        let connection = Connection::open(&self.database_path)
            .map_err(|error| BotError::Database(error.to_string()))?;
        connection
            .execute(
                "CREATE TABLE IF NOT EXISTS telegram_chat_model_selection (chat_id INTEGER PRIMARY KEY, model TEXT NOT NULL)",
                [],
            )
            .map_err(|error| BotError::Database(error.to_string()))?;
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
            .prepare("SELECT model FROM telegram_chat_model_selection WHERE chat_id = ?1")
            .map_err(|error| BotError::Database(error.to_string()))?;
        let mut rows = statement
            .query(params![chat_id])
            .map_err(|error| BotError::Database(error.to_string()))?;

        let row = rows
            .next()
            .map_err(|error| BotError::Database(error.to_string()))?;

        match row {
            Some(row) => row
                .get::<_, String>(0)
                .map(Some)
                .map_err(|error| BotError::Database(error.to_string())),
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
            .map_err(|error| BotError::Database(error.to_string()))?;
        Ok(())
    }

    fn clear_selected_model(&self, chat_id: i64) -> Result<(), BotError> {
        let connection = self.open_connection()?;
        connection
            .execute(
                "DELETE FROM telegram_chat_model_selection WHERE chat_id = ?1",
                params![chat_id],
            )
            .map_err(|error| BotError::Database(error.to_string()))?;
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

        for model in models.into_iter().chain(std::iter::once(ModelChoice::new(default_model.clone()))) {
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
            Arc::new(StaticModelCatalog::new("gpt-5.4", vec![ModelChoice::new("gpt-5.4")])),
            Arc::new(InMemoryModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        );

        assert_eq!(service.current_model(1).unwrap(), "gpt-5.4");
    }
}
