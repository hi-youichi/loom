//! Dataset storage: JSONL-based with train/val/holdout splits.

use crate::types::{EvalExample, Split};
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;

/// File-system backed dataset store.
pub struct FsDatasetStore {
    base_dir: std::path::PathBuf,
}

impl FsDatasetStore {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Load all examples from the store directory (reads train.jsonl, val.jsonl, holdout.jsonl).
    pub fn load_all(&self) -> Result<Vec<(EvalExample, Split)>, DatasetError> {
        let mut result = Vec::new();
        for split in [Split::Train, Split::Val, Split::Holdout] {
            let path = self.split_path(split);
            if path.exists() {
                let examples = Self::read_jsonl(&path)?;
                for ex in examples {
                    result.push((ex, split));
                }
            }
        }
        Ok(result)
    }

    /// Load examples for a specific split.
    pub fn load_split(&self, split: Split) -> Result<Vec<EvalExample>, DatasetError> {
        let path = self.split_path(split);
        if !path.exists() {
            return Ok(Vec::new());
        }
        Self::read_jsonl(&path)
    }

    /// Save examples to their respective split files.
    pub fn save_all(&self, examples: &[(EvalExample, Split)]) -> Result<(), DatasetError> {
        fs::create_dir_all(&self.base_dir)?;
        for split in [Split::Train, Split::Val, Split::Holdout] {
            let subset: Vec<&EvalExample> = examples
                .iter()
                .filter(|(_, s)| *s == split)
                .map(|(e, _)| e)
                .collect();
            if !subset.is_empty() {
                let path = self.split_path(split);
                Self::write_jsonl(&path, &subset)?;
            }
        }
        Ok(())
    }

    /// Split raw examples into train/val/holdout and save.
    /// Ratios: 60% train, 20% val, 20% holdout.
    pub fn split_and_save(
        &self,
        examples: Vec<EvalExample>,
    ) -> Result<Vec<(EvalExample, Split)>, DatasetError> {
        let mut rng = thread_rng();
        let mut shuffled = examples;
        shuffled.shuffle(&mut rng);

        let total = shuffled.len();
        let train_end = (total as f64 * 0.6).round() as usize;
        let val_end = train_end + (total as f64 * 0.2).round() as usize;

        let split_data: Vec<(EvalExample, Split)> = shuffled
            .into_iter()
            .enumerate()
            .map(|(i, ex)| {
                let split = if i < train_end {
                    Split::Train
                } else if i < val_end {
                    Split::Val
                } else {
                    Split::Holdout
                };
                (ex, split)
            })
            .collect();

        self.save_all(&split_data)?;
        Ok(split_data)
    }

    fn split_path(&self, split: Split) -> std::path::PathBuf {
        let name = match split {
            Split::Train => "train.jsonl",
            Split::Val => "val.jsonl",
            Split::Holdout => "holdout.jsonl",
        };
        self.base_dir.join(name)
    }

    fn read_jsonl(path: &Path) -> Result<Vec<EvalExample>, DatasetError> {
        let file = fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let mut examples = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let ex: EvalExample = serde_json::from_str(&line)?;
            examples.push(ex);
        }
        Ok(examples)
    }

    fn write_jsonl(path: &Path, examples: &[&EvalExample]) -> Result<(), DatasetError> {
        let mut file = fs::File::create(path)?;
        for ex in examples {
            let json = serde_json::to_string(ex)?;
            writeln!(file, "{}", json)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DatasetError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Generate synthetic eval examples from a skill file using an LLM prompt.
pub fn generate_synthetic_prompt(skill_content: &str, n: usize) -> String {
    format!(
        r#"你是一个评估数据集构建专家。阅读以下技能文件，生成 {n} 个测试样本。

每个样本是一行 JSON，格式为：
{{"task_input": "...", "expected_behavior": "...", "difficulty": "Easy|Medium|Hard"}}

规则：
- task_input：用户可能给出的真实任务描述
- expected_behavior：评分标准（rubric），描述正确执行应该达到什么效果，而非精确文本
- difficulty：Easy / Medium / Hard，大致均匀分布

请直接输出 {n} 行 JSON，不要其他内容。

技能文件：
{skill_content}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Difficulty;

    #[test]
    fn split_and_save_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsDatasetStore::new(dir.path());

        let examples: Vec<EvalExample> = (0..10)
            .map(|i| EvalExample {
                task_input: format!("task {}", i),
                expected_behavior: format!("expected {}", i),
                difficulty: if i % 3 == 0 {
                    Difficulty::Easy
                } else if i % 3 == 1 {
                    Difficulty::Medium
                } else {
                    Difficulty::Hard
                },
            })
            .collect();

        let split = store.split_and_save(examples).unwrap();
        assert_eq!(split.len(), 10);

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 10);
    }

    #[test]
    fn load_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsDatasetStore::new(dir.path());
        let result = store.load_all().unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn generate_prompt_contains_skill() {
        let prompt = generate_synthetic_prompt("my skill content", 5);
        assert!(prompt.contains("my skill content"));
        assert!(prompt.contains("5"));
    }
}
