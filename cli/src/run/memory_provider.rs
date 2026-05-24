use crate::run::memory::{MemoryFile, MemoryStore};
use std::path::Path;

pub trait MemoryProvider: Send + Sync {
    fn load(&self, file: MemoryFile) -> Result<String, MemoryProviderError>;
    fn append(&self, file: MemoryFile, content: &str) -> Result<(), MemoryProviderError>;
    fn replace(&self, file: MemoryFile, content: &str) -> Result<(), MemoryProviderError>;
    fn load_all_for_prompt(&self) -> Result<String, MemoryProviderError>;
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

impl MemoryProvider for LocalFileProvider {
    fn load(&self, file: MemoryFile) -> Result<String, MemoryProviderError> {
        self.store.load(file).map_err(|e| MemoryProviderError::Io(std::io::Error::other(e.to_string())))
    }

    fn append(&self, file: MemoryFile, content: &str) -> Result<(), MemoryProviderError> {
        self.store.append(file, content).map_err(|e| MemoryProviderError::Io(std::io::Error::other(e.to_string())))
    }

    fn replace(&self, file: MemoryFile, content: &str) -> Result<(), MemoryProviderError> {
        self.store.replace(file, content).map_err(|e| MemoryProviderError::Io(std::io::Error::other(e.to_string())))
    }

    fn load_all_for_prompt(&self) -> Result<String, MemoryProviderError> {
        self.store.load_all_for_prompt().map_err(|e| MemoryProviderError::Io(std::io::Error::other(e.to_string())))
    }

    fn name(&self) -> &'static str {
        "local_file"
    }
}

pub struct NoopProvider;

impl MemoryProvider for NoopProvider {
    fn load(&self, _file: MemoryFile) -> Result<String, MemoryProviderError> {
        Ok(String::new())
    }

    fn append(&self, _file: MemoryFile, _content: &str) -> Result<(), MemoryProviderError> {
        Ok(())
    }

    fn replace(&self, _file: MemoryFile, _content: &str) -> Result<(), MemoryProviderError> {
        Ok(())
    }

    fn load_all_for_prompt(&self) -> Result<String, MemoryProviderError> {
        Ok(String::new())
    }

    fn name(&self) -> &'static str {
        "noop"
    }
}

pub fn default_provider() -> Box<dyn MemoryProvider> {
    Box::new(LocalFileProvider::new(&MemoryStore::default_path()))
}
