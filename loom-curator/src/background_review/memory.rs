use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryFile {
    User,
    Project,
    Facts,
}

impl MemoryFile {
    pub fn filename(&self) -> &'static str {
        match self {
            MemoryFile::User => "USER.md",
            MemoryFile::Project => "PROJECT.md",
            MemoryFile::Facts => "FACTS.md",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMatch {
    pub file: MemoryFile,
    pub line_number: usize,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_max_memory_chars")]
    pub max_memory_chars: usize,
}

fn default_max_chars() -> usize {
    8000
}
fn default_max_memory_chars() -> usize {
    4000
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_chars: default_max_chars(),
            max_memory_chars: default_max_memory_chars(),
        }
    }
}

pub struct MemoryStore {
    base_dir: PathBuf,
    config: MemoryConfig,
}

impl MemoryStore {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
            config: MemoryConfig::default(),
        }
    }

    pub fn with_config(base_dir: &Path, config: MemoryConfig) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
            config,
        }
    }

    pub fn default_path() -> PathBuf {
        env_config::home::loom_home().join("data").join("memory")
    }

    fn file_path(&self, file: MemoryFile) -> PathBuf {
        self.base_dir.join(file.filename())
    }

    pub fn load(&self, file: MemoryFile) -> Result<String, MemoryError> {
        let path = self.file_path(file);
        if !path.exists() {
            return Ok(String::new());
        }
        Ok(fs::read_to_string(&path)?)
    }

    pub fn append(&self, file: MemoryFile, content: &str) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.base_dir)?;
        let path = self.file_path(file);
        let existing = self.load(file)?;
        let separator = if existing.is_empty() { "" } else { "\n\n" };
        let new_content = format!("{}{}{}", existing, separator, content);
        fs::write(&path, &new_content)?;
        self.truncate_to_limit(file, self.config.max_chars)?;
        Ok(())
    }

    pub fn replace(&self, file: MemoryFile, content: &str) -> Result<(), MemoryError> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(MemoryError::EmptyContent);
        }
        fs::create_dir_all(&self.base_dir)?;
        let path = self.file_path(file);
        // Backup existing content before overwriting
        if path.exists() {
            if let Ok(existing) = fs::read_to_string(&path) {
                if !existing.trim().is_empty() {
                    let backup_dir = self.base_dir.join("backups");
                    let _ = fs::create_dir_all(&backup_dir);
                    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
                    let backup_path = backup_dir.join(format!("{}_{}.md", file.filename().trim_end_matches(".md"), timestamp));
                    let _ = fs::write(&backup_path, &existing);
                }
            }
        }
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn search(&self, query: &str) -> Result<Vec<MemoryMatch>, MemoryError> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        for file in [MemoryFile::User, MemoryFile::Project, MemoryFile::Facts] {
            let content = self.load(file)?;
            for (i, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&query_lower) {
                    results.push(MemoryMatch {
                        file,
                        line_number: i + 1,
                        line: line.to_string(),
                    });
                }
            }
        }
        Ok(results)
    }

    pub fn truncate_to_limit(&self, file: MemoryFile, max_chars: usize) -> Result<(), MemoryError> {
        let content = self.load(file)?;
        if content.len() <= max_chars {
            return Ok(());
        }

        let sections: Vec<&str> = content.splitn(2, "---\n").collect();
        let (frontmatter, body) = if sections.len() == 2 && sections[0].trim().is_empty() {
            ("---\n", sections[1])
        } else if sections.len() == 2 {
            (sections[0], sections[1])
        } else {
            ("", content.as_str())
        };

        let entries: Vec<&str> = body.split("\n---\n").collect();
        let frontmatter_len = frontmatter.len();
        let mut kept = Vec::new();
        let mut total_len = frontmatter_len;

        for entry in entries.iter().rev() {
            let _entry_with_sep = if kept.is_empty() {
                entry.to_string()
            } else {
                format!("{}\n---\n{}", entry, kept.last().unwrap())
            };
            if total_len + entry.len() + 5 <= max_chars {
                kept.insert(0, *entry);
                total_len += entry.len() + 5;
            } else {
                break;
            }
        }

        let result = format!("{}{}", frontmatter, kept.join("\n---\n"));
        let path = self.file_path(file);
        fs::write(&path, &result)?;
        Ok(())
    }

    pub fn load_all_for_prompt(&self) -> Result<String, MemoryError> {
        let files = [
            (MemoryFile::Facts, "## Facts"),
            (MemoryFile::Project, "## Project"),
            (MemoryFile::User, "## User"),
        ];

        let mut parts = Vec::new();
        let mut total_len = 0;

        for (file, header) in &files {
            let content = self.load(*file)?;
            if !content.trim().is_empty() {
                let section = format!("{}\n{}", header, content);
                if total_len + section.len() <= self.config.max_memory_chars {
                    total_len += section.len();
                    parts.push(section);
                } else if parts.is_empty() {
                    let truncated: String = content.chars().take(self.config.max_memory_chars).collect();
                    parts.push(format!("{}\n{}", header, truncated));
                    break;
                }
            }
        }

        Ok(parts.join("\n\n"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("content cannot be empty")]
    EmptyContent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store.append(MemoryFile::User, "prefers Rust").unwrap();
        store.append(MemoryFile::User, "uses vim").unwrap();
        let content = store.load(MemoryFile::User).unwrap();
        assert!(content.contains("prefers Rust"));
        assert!(content.contains("uses vim"));
    }

    #[test]
    fn replace_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store.append(MemoryFile::User, "old").unwrap();
        store.replace(MemoryFile::User, "new").unwrap();
        assert_eq!(store.load(MemoryFile::User).unwrap(), "new");
    }

    #[test]
    fn search_finds_matches() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store.replace(MemoryFile::User, "likes Rust\nhates bugs").unwrap();
        let matches = store.search("rust").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].file, MemoryFile::User);
    }

    #[test]
    fn truncate_removes_oldest_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        let long_content = (0..10)
            .map(|i| format!("Entry {} with some padding text here", i))
            .collect::<Vec<_>>()
            .join("\n---\n");
        store.replace(MemoryFile::User, &long_content).unwrap();
        store.truncate_to_limit(MemoryFile::User, 100).unwrap();
        let result = store.load(MemoryFile::User).unwrap();
        assert!(result.len() <= 100);
    }

    #[test]
    fn replace_rejects_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store.append(MemoryFile::User, "important data").unwrap();
        let result = store.replace(MemoryFile::User, "");
        assert!(result.is_err());
        // Original content should be preserved
        assert_eq!(store.load(MemoryFile::User).unwrap(), "important data");
    }

    #[test]
    fn replace_rejects_whitespace_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store.append(MemoryFile::User, "important data").unwrap();
        let result = store.replace(MemoryFile::User, "   \n\t  ");
        assert!(result.is_err());
        assert_eq!(store.load(MemoryFile::User).unwrap(), "important data");
    }

    #[test]
    fn replace_creates_backup() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        store.replace(MemoryFile::User, "old content").unwrap();
        store.replace(MemoryFile::User, "new content").unwrap();
        assert_eq!(store.load(MemoryFile::User).unwrap(), "new content");
        let backup_dir = dir.path().join("backups");
        assert!(backup_dir.exists());
        let backups: Vec<_> = std::fs::read_dir(&backup_dir).unwrap().collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path());
        assert_eq!(store.load(MemoryFile::User).unwrap(), "");
    }

    #[test]
    fn load_all_for_prompt_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let config = MemoryConfig {
            max_chars: 10000,
            max_memory_chars: 50,
        };
        let store = MemoryStore::with_config(dir.path(), config);
        store.replace(MemoryFile::Facts, &"a".repeat(100)).unwrap();
        let prompt = store.load_all_for_prompt().unwrap();
        assert!(prompt.len() <= 80);
    }
}
