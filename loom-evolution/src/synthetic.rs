use crate::dataset::FsDatasetStore;
use crate::types::{Difficulty, EvalExample};
use tracing::{info, warn};

pub async fn generate_dataset(
    llm: &dyn crate::optimizer::EvolutionLlm,
    skill_content: &str,
    count: usize,
) -> Result<Vec<EvalExample>, Box<dyn std::error::Error + Send + Sync>> {
    let prompt = format!(
        r#"你是一个测试用例生成器。根据以下技能文件，生成 {count} 个多样化的测试用例。

每个用例包含：
- task_input: 用户可能给出的任务描述（多样化，覆盖边界情况）
- expected_behavior: 评分标准（rubric），不是精确输出
- difficulty: Easy / Medium / Hard

## 技能文件
{skill_content}

输出 JSON 数组，格式如下：
[
  {{"task_input": "...", "expected_behavior": "...", "difficulty": "Easy"}},
  ...
]

直接输出 JSON，不要其他内容。"#,
        count = count,
        skill_content = skill_content,
    );

    let mut retries = 0;
    let response = loop {
        match llm.complete(&prompt).await {
            Ok(r) => break r,
            Err(e) => {
                retries += 1;
                if retries >= 3 {
                    return Err(e);
                }
                let delay = std::time::Duration::from_secs(2u64.pow(retries));
                warn!("LLM call failed (attempt {}), retrying in {:?}: {}", retries, delay, e);
                tokio::time::sleep(delay).await;
            }
        }
    };

    let examples = parse_dataset_response(&response, count)?;
    info!("Generated {} synthetic examples", examples.len());
    Ok(examples)
}

fn parse_dataset_response(
    response: &str,
    expected: usize,
) -> Result<Vec<EvalExample>, Box<dyn std::error::Error + Send + Sync>> {
    let json_str = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    #[derive(serde::Deserialize)]
    struct RawExample {
        task_input: String,
        expected_behavior: String,
        #[serde(default)]
        difficulty: Option<String>,
    }

    let raw: Vec<RawExample> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => {
            let mut examples = Vec::new();
            for line in json_str.lines() {
                let line = line.trim();
                if line.is_empty() || !line.starts_with('{') {
                    continue;
                }
                if let Ok(ex) = serde_json::from_str::<RawExample>(line) {
                    examples.push(ex);
                }
            }
            if examples.is_empty() {
                return Err(format!("Failed to parse any examples from response").into());
            }
            examples
        }
    };

    let examples: Vec<EvalExample> = raw
        .into_iter()
        .take(expected)
        .map(|r| {
            let difficulty = match r.difficulty.as_deref() {
                Some("Easy") | Some("easy") => Difficulty::Easy,
                Some("Hard") | Some("hard") => Difficulty::Hard,
                _ => Difficulty::Medium,
            };
            EvalExample {
                task_input: r.task_input,
                expected_behavior: r.expected_behavior,
                difficulty,
            }
        })
        .collect();

    Ok(examples)
}

pub async fn generate_and_save(
    llm: &dyn crate::optimizer::EvolutionLlm,
    skill_content: &str,
    count: usize,
    dataset_dir: &std::path::Path,
) -> Result<Vec<EvalExample>, Box<dyn std::error::Error + Send + Sync>> {
    let examples = generate_dataset(llm, skill_content, count).await?;
    let store = FsDatasetStore::new(dataset_dir);
    store.split_and_save(examples.clone())?;
    Ok(examples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_json_array() {
        let response = r#"[
            {"task_input": "Fix bug", "expected_behavior": "Should fix", "difficulty": "Easy"},
            {"task_input": "Refactor", "expected_behavior": "Should refactor", "difficulty": "Hard"}
        ]"#;
        let examples = parse_dataset_response(response, 10).unwrap();
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].difficulty, Difficulty::Easy);
        assert_eq!(examples[1].difficulty, Difficulty::Hard);
    }

    #[test]
    fn parse_jsonl_fallback() {
        let response = r#"{"task_input": "A", "expected_behavior": "B", "difficulty": "Medium"}
{"task_input": "C", "expected_behavior": "D"}"#;
        let examples = parse_dataset_response(response, 10).unwrap();
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[1].difficulty, Difficulty::Medium);
    }

    #[test]
    fn parse_respects_count_limit() {
        let response = r#"[
            {"task_input": "A", "expected_behavior": "B"},
            {"task_input": "C", "expected_behavior": "D"},
            {"task_input": "E", "expected_behavior": "F"}
        ]"#;
        let examples = parse_dataset_response(response, 2).unwrap();
        assert_eq!(examples.len(), 2);
    }

    #[test]
    fn parse_with_markdown_wrapper() {
        let response = "```json\n[{\"task_input\": \"A\", \"expected_behavior\": \"B\"}]\n```";
        let examples = parse_dataset_response(response, 10).unwrap();
        assert_eq!(examples.len(), 1);
    }
}
