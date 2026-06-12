use crate::run::memory::{MemoryFile, MemoryStore, MemoryError};
use std::path::Path;

pub trait MemoryProvider: Send + Sync {
    fn load(&self, file: MemoryFile) -> Result<String, MemoryProviderError>;
    fn add_entry(&self, file: MemoryFile, content: &str) -> Result<String, MemoryProviderError>;
    fn replace_entry(&self, file: MemoryFile, old_text: &str, new_content: &str) -> Result<String, MemoryProviderError>;
    fn remove_entry(&self, file: MemoryFile, old_text: &str) -> Result<String, MemoryProviderError>;
    fn read_entries(&self, file: MemoryFile) -> Result<Vec<String>, MemoryProviderError>;
    fn load_all_for_prompt(&self) -> Result<String, MemoryProviderError>;
    fn capture_snapshot(&self) -> Result<String, MemoryProviderError>;
    fn name(&self) -> &'static str;
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryProviderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("API error: {0}")]
    Api(String),
    #[error("Config error: {0}")]
    Config(String),
}

pub struct LocalFileProvider {
    store: MemoryStore,
}

impl LocalFileProvider {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            store: MemoryStore::new(base_dir),
        }
    }
}

fn map_err(e: MemoryError) -> MemoryProviderError {
    MemoryProviderError::Io(std::io::Error::other(e.to_string()))
}

impl MemoryProvider for LocalFileProvider {
    fn load(&self, file: MemoryFile) -> Result<String, MemoryProviderError> {
        self.store.load(file).map_err(map_err)
    }

    fn add_entry(&self, file: MemoryFile, content: &str) -> Result<String, MemoryProviderError> {
        let result = self.store.add_entry(file, content).map_err(map_err)?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_default())
    }

    fn replace_entry(&self, file: MemoryFile, old_text: &str, new_content: &str) -> Result<String, MemoryProviderError> {
        let result = self.store.replace_entry(file, old_text, new_content).map_err(map_err)?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_default())
    }

    fn remove_entry(&self, file: MemoryFile, old_text: &str) -> Result<String, MemoryProviderError> {
        let result = self.store.remove_entry(file, old_text).map_err(map_err)?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_default())
    }

    fn read_entries(&self, file: MemoryFile) -> Result<Vec<String>, MemoryProviderError> {
        self.store.read_entries(file).map_err(map_err)
    }

    fn load_all_for_prompt(&self) -> Result<String, MemoryProviderError> {
        self.store.load_all_for_prompt().map_err(map_err)
    }

    fn capture_snapshot(&self) -> Result<String, MemoryProviderError> {
        self.store.capture_snapshot().map_err(map_err)
    }

    fn name(&self) -> &'static str {
        "local_file"
    }
}
