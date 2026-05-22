use crate::dataset::FsDatasetStore;
use crate::optimizer::{EvolutionLlm, retry_llm_call};
use crate::types::{Difficulty, EvalExample};
use tracing::{info, warn};

pub trait SessionStore: Send + Sync {
    fn search_sessions(&self, query: &str, limit: usize) -> Result<Vec<SessionInfo>, String>;
    fn get_session_content(&self, session_id: &str) -> Result<String, String>;
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub title: Option<String>,
}

pub async fn mine_from_sessions(
    store: &dyn SessionStore,
    skill_name: &str,
    skill_triggers: &[String],
    llm: &dyn EvolutionLlm,
    max_samples: usize,
) -> Result<Vec<EvalExample>, Box<dyn std::error::Error + Send + Sync>> {
    let mut all_examples = Vec::new();

    for trigger in skill_triggers {
        let sessions = store.search_sessions(trigger, 20)?;
        for session in sessions {
            if all_examples.len() >= max_samples {
                break;
            }

            let content = match store.get_session_content(&session.id) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to get session {}: {}", session.id, e);
                    continue;
                }
            };

            match extract_examples(llm, &content, skill_name, 3).await {
                Ok(examples) => {
                    all_examples.extend(examples);
                }
                Err(e) => {
                    warn!("Failed to extract examples from session {}: {}", session.id, e);
                }
            }
        }
        if all_examples.len() >= max_samples {
            break;
        }
    }

    all_examples.truncate(max_samples);
    info!("Mined {} examples for skill '{}'", all_examples.len(), skill_name);
    Ok(all_examples)
}

async fn extract_examples(
    llm: &dyn EvolutionLlm,
    session_content: &str,
    skill_name: &str,
    max: usize,
) -> Result<Vec<EvalExample>, Box<dyn std::error::Error + Send + Sync>> {
    let truncated = if session_content.len() > 8000 {
        &session_content[..8000]
    } else {
        session_content
    };

    let prompt = format!(
        r#"分析以下会话，提取与技能「{skill_name}」相关的测试用例。

每个用例包含：
- task_input: 用户给出的任务描述
- expected_behavior: 期望的执行标准（rubric）
- difficulty: Easy / Medium / Hard

## 会话内容
{truncated}

输出 JSON 数组，最多 {max} 个：
[
  {{"task_input": "...", "expected_behavior": "...", "difficulty": "Medium"}}
]

如果没有相关内容，输出空数组 []。直接输出 JSON，不要其他内容。"#,
    );

    let response = retry_llm_call(llm, &prompt).await?;
    parse_mined_examples(&response, max)
}

fn parse_mined_examples(
    response: &str,
    max: usize,
) -> Result<Vec<EvalExample>, Box<dyn std::error::Error + Send + Sync>> {
    let json_str = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    #[derive(serde::Deserialize)]
    struct Raw {
        task_input: String,
        expected_behavior: String,
        difficulty: Option<String>,
    }

    let raw: Vec<Raw> = serde_json::from_str(json_str).unwrap_or_else(|_| {
        json_str
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.starts_with('{') {
                    serde_json::from_str::<Raw>(line).ok()
                } else {
                    None
                }
            })
            .collect()
    });

    Ok(raw
        .into_iter()
        .take(max)
        .map(|r| EvalExample {
            task_input: r.task_input,
            expected_behavior: r.expected_behavior,
            difficulty: match r.difficulty.as_deref() {
                Some("Easy") | Some("easy") => Difficulty::Easy,
                Some("Hard") | Some("hard") => Difficulty::Hard,
                _ => Difficulty::Medium,
            },
        })
        .collect())
}

pub async fn mine_and_save(
    store: &dyn SessionStore,
    skill_name: &str,
    skill_triggers: &[String],
    llm: &dyn EvolutionLlm,
    max_samples: usize,
    dataset_dir: &std::path::Path,
) -> Result<Vec<EvalExample>, Box<dyn std::error::Error + Send + Sync>> {
    let examples = mine_from_sessions(store, skill_name, skill_triggers, llm, max_samples).await?;
    if !examples.is_empty() {
        let store = FsDatasetStore::new(dataset_dir);
        store.split_and_save(examples.clone())?;
    }
    Ok(examples)
}
