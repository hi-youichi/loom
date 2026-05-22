use crate::run::memory::{MemoryFile, MemoryStore};
use crate::run::skill_registry::{Lifecycle, SkillContent, SkillRegistry, Source};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewOutput {
    #[serde(default)]
    pub memory_updates: Vec<MemoryUpdate>,
    #[serde(default)]
    pub skill_suggestions: Vec<SkillSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUpdate {
    pub file: String,
    pub action: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSuggestion {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    pub body: String,
}

pub trait ReviewLlm: Send + Sync {
    fn complete(&self, prompt: &str) -> Result<String, String>;
}

pub struct ReviewConfig {
    pub auto_create_threshold: usize,
    pub max_session_chars: usize,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            auto_create_threshold: 5,
            max_session_chars: 12000,
        }
    }
}

pub struct ReviewAgent<'a> {
    llm: &'a dyn ReviewLlm,
    memory: &'a MemoryStore,
    skills: &'a SkillRegistry,
    config: ReviewConfig,
}

impl<'a> ReviewAgent<'a> {
    pub fn new(
        llm: &'a dyn ReviewLlm,
        memory: &'a MemoryStore,
        skills: &'a SkillRegistry,
    ) -> Self {
        Self {
            llm,
            memory,
            skills,
            config: ReviewConfig::default(),
        }
    }

    pub fn with_config(
        llm: &'a dyn ReviewLlm,
        memory: &'a MemoryStore,
        skills: &'a SkillRegistry,
        config: ReviewConfig,
    ) -> Self {
        Self {
            llm,
            memory,
            skills,
            config,
        }
    }

    pub fn review_session(&self, session_content: &str) -> Result<ReviewOutput, String> {
        let truncated = if session_content.len() > self.config.max_session_chars {
            &session_content[..self.config.max_session_chars]
        } else {
            session_content
        };

        let prompt = build_review_prompt(truncated);
        let mut retries = 0;
        let response = loop {
            match self.llm.complete(&prompt) {
                Ok(r) => break r,
                Err(e) => {
                    retries += 1;
                    if retries >= 3 {
                        return Err(format!(
                            "Review LLM call failed after {} retries: {}",
                            retries, e
                        ));
                    }
                    warn!("Review LLM call failed (attempt {}): {}", retries, e);
                }
            }
        };

        let output = parse_review_response(&response)?;
        self.apply_memory_updates(&output.memory_updates)?;
        self.apply_skill_suggestions(&output.skill_suggestions)?;
        Ok(output)
    }

    fn apply_memory_updates(&self, updates: &[MemoryUpdate]) -> Result<(), String> {
        for update in updates {
            let file = match update.file.to_lowercase().as_str() {
                "user" | "user.md" => MemoryFile::User,
                "project" | "project.md" => MemoryFile::Project,
                "facts" | "facts.md" => MemoryFile::Facts,
                _ => {
                    warn!("Unknown memory file in review output: {}", update.file);
                    continue;
                }
            };

            match update.action.to_lowercase().as_str() {
                "append" => {
                    self.memory.append(file, &update.content).map_err(|e| e.to_string())?;
                    info!("Appended to {:?}: {} chars", file, update.content.len());
                }
                "replace" => {
                    self.memory.replace(file, &update.content).map_err(|e| e.to_string())?;
                    info!("Replaced {:?}: {} chars", file, update.content.len());
                }
                other => {
                    warn!("Unknown memory action: {}", other);
                }
            }
        }
        Ok(())
    }

    fn apply_skill_suggestions(&self, suggestions: &[SkillSuggestion]) -> Result<(), String> {
        for suggestion in suggestions {
            match self.skills.load(&suggestion.name) {
                Ok(existing) => {
                    info!(
                        "Skill '{}' already exists (lifecycle: {:?}), skipping auto-creation",
                        suggestion.name, existing.lifecycle
                    );
                }
                Err(_) => {
                    let skill = SkillContent {
                        name: suggestion.name.clone(),
                        description: suggestion.description.clone(),
                        triggers: suggestion.triggers.clone(),
                        lifecycle: Lifecycle::Active,
                        source: Source::Auto,
                        body: suggestion.body.clone(),
                        raw: String::new(),
                    };
                    self.skills.save(&suggestion.name, &skill).map_err(|e| e.to_string())?;
                    info!("Auto-created skill: {}", suggestion.name);
                }
            }
        }
        Ok(())
    }
}

fn build_review_prompt(session_content: &str) -> String {
    format!(
        r#"你是 Loom 的审查 Agent。分析以下对话，提取有用的信息。

## 任务
1. **用户偏好**: 语言、工具、编码风格等 (写入 USER.md)
2. **项目事实**: 技术栈、架构决策、关键路径等 (写入 PROJECT.md)
3. **事实记录**: API 端点、配置值、版本号等 (写入 FACTS.md)
4. **可复用模式**: 如果发现重复的工作模式，建议创建技能

## 会话内容
{session_content}

## 输出格式（严格 JSON，不要其他内容）
{{
  "memory_updates": [
    {{"file": "USER", "action": "append", "content": "用户偏好..."}},
    {{"file": "PROJECT", "action": "append", "content": "项目事实..."}},
    {{"file": "FACTS", "action": "append", "content": "关键事实..."}}
  ],
  "skill_suggestions": [
    {{
      "name": "skill-name",
      "description": "技能描述",
      "triggers": ["触发关键词1", "关键词2"],
      "body": "技能的具体步骤..."
    }}
  ]
}}

注意：
- 只输出 JSON，不要其他文字
- memory_updates 中的 file 只能是 USER、PROJECT 或 FACTS
- action 只能是 append 或 replace
- 没有需要记录的内容时，对应数组为空
- skill_suggestions 只在有明确可复用模式时才填写"#,
    )
}

fn parse_review_response(response: &str) -> Result<ReviewOutput, String> {
    let json_str = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    match serde_json::from_str::<ReviewOutput>(json_str) {
        Ok(output) => Ok(output),
        Err(e) => {
            if let Some(start) = json_str.find('{') {
                if let Some(end) = json_str.rfind('}') {
                    let sub = &json_str[start..=end];
                    serde_json::from_str::<ReviewOutput>(sub)
                        .map_err(|e2| format!("Failed to parse review JSON: {} | {}", e, e2))
                } else {
                    Err(format!("Failed to parse review JSON: {}", e))
                }
            } else {
                Err(format!("No JSON found in review response: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct MockLlm {
        response: String,
        call_count: std::sync::Mutex<usize>,
    }

    impl MockLlm {
        fn new(response: &str) -> Self {
            Self {
                response: response.to_string(),
                call_count: std::sync::Mutex::new(0),
            }
        }
    }

    impl ReviewLlm for MockLlm {
        fn complete(&self, _prompt: &str) -> Result<String, String> {
            *self.call_count.lock().unwrap() += 1;
            Ok(self.response.clone())
        }
    }

    fn make_valid_response() -> String {
        r#"{
            "memory_updates": [
                {"file": "USER", "action": "append", "content": "prefers Rust"},
                {"file": "FACTS", "action": "append", "content": "uses tokio runtime"}
            ],
            "skill_suggestions": [
                {
                    "name": "debug-rust-errors",
                    "description": "Debug Rust compiler errors",
                    "triggers": ["rust", "compiler error", "cargo build"],
                    "body": "1. Read error\n2. Fix\n"
                }
            ]
        }"#.to_string()
    }

    #[test]
    fn parse_valid_response() {
        let response = make_valid_response();
        let output = parse_review_response(&response).unwrap();
        assert_eq!(output.memory_updates.len(), 2);
        assert_eq!(output.skill_suggestions.len(), 1);
        assert_eq!(output.skill_suggestions[0].name, "debug-rust-errors");
    }

    #[test]
    fn parse_response_with_markdown() {
        let response = format!("```json\n{}\n```", make_valid_response());
        let output = parse_review_response(&response).unwrap();
        assert_eq!(output.memory_updates.len(), 2);
    }

    #[test]
    fn parse_empty_arrays() {
        let response = r#"{"memory_updates": [], "skill_suggestions": []}"#;
        let output = parse_review_response(response).unwrap();
        assert!(output.memory_updates.is_empty());
        assert!(output.skill_suggestions.is_empty());
    }

    #[test]
    fn full_review_flow() {
        let dir = tempfile::tempdir().unwrap();
        let memory = MemoryStore::new(dir.path());
        let skills_dir = dir.path().join("skills");
        let skills = SkillRegistry::new(&skills_dir);

        let llm = MockLlm::new(&make_valid_response());
        let agent = ReviewAgent::new(&llm, &memory, &skills);

        let session = "User asked to fix a Rust compiler error in main.rs";
        let output = agent.review_session(session).unwrap();

        assert_eq!(output.memory_updates.len(), 2);
        assert_eq!(output.skill_suggestions.len(), 1);

        let user_content = memory.load(MemoryFile::User).unwrap();
        assert!(user_content.contains("prefers Rust"));

        let loaded_skill = skills.load("debug-rust-errors").unwrap();
        assert_eq!(loaded_skill.source, Source::Auto);
    }
}
