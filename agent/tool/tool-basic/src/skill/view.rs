//! Skill view tool — loads a skill's full content by name.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use skill::utils::ReadinessStatus;
use skill::SkillRegistry;
use tool_core::Tool;
use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError};

use super::SkillContext;

pub use tool_core::tool_name::TOOL_SKILL_VIEW;

pub struct SkillViewTool {
    ctx: Arc<SkillContext>,
}

impl SkillViewTool {
    pub(crate) fn new(ctx: Arc<SkillContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for SkillViewTool {
    fn name(&self) -> &str {
        TOOL_SKILL_VIEW
    }

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
            name: TOOL_SKILL_VIEW.to_string(),
            description: Some(
                "Load a skill's full content by name. Use when a task matches one of the \
                 available skills. Optionally specify file_path to read a sub-file within \
                 the skill directory (e.g. \"references/api.md\", \"scripts/setup.sh\")."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Skill name to load (from skill_list or <available_skills>)."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Optional sub-file path within the skill directory \
                                        (e.g. \"references/api.md\", \"scripts/setup.sh\")."
                    }
                },
                "required": ["name"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing name".to_string()))?;

        let file_path = args.get("file_path").and_then(|v| v.as_str());

        if let Some(ref registry) = self.ctx.registry {
            return self.view_from_registry(registry, name, file_path);
        }

        self.view_from_directory(name, file_path)
    }
}

impl SkillViewTool {
    fn resolve_name_with_ns<'a>(
        &self,
        name: &str,
        registry: &'a SkillRegistry,
    ) -> Result<Vec<&'a skill::SkillEntry>, ToolSourceError> {
        let matches: Vec<&'a skill::SkillEntry> =
            if let Some((ns, short_name)) = name.split_once(':') {
                registry
                    .list()
                    .iter()
                    .filter(|e| {
                        e.metadata.name == short_name && e.base_path.to_string_lossy().contains(ns)
                    })
                    .collect()
            } else {
                registry
                    .list()
                    .iter()
                    .filter(|e| e.metadata.name == name)
                    .collect()
            };
        Ok(matches)
    }

    fn view_from_registry(
        &self,
        registry: &SkillRegistry,
        name: &str,
        file_path: Option<&str>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let matches = self.resolve_name_with_ns(name, registry)?;

        if matches.is_empty() {
            let available: Vec<&str> = registry
                .list()
                .iter()
                .map(|e| e.metadata.name.as_str())
                .collect();
            return Err(ToolSourceError::InvalidInput(format!(
                "skill '{}' not found. Available: {}",
                name,
                available.join(", ")
            )));
        }

        if matches.len() > 1 {
            let sources: Vec<String> = matches
                .iter()
                .map(|e| format!("{} ({:?})", e.base_path.display(), e.source))
                .collect();
            return Err(ToolSourceError::InvalidInput(format!(
                "Ambiguous skill '{}': found in {} locations. Use a more specific name or path.\n  {}",
                name,
                matches.len(),
                sources.join("\n  ")
)));
        }

        let entry = matches[0];
        let base_path = &entry.base_path;

        if let Some(fp) = file_path {
            if entry.embedded_content.is_some() {
                return Err(ToolSourceError::InvalidInput(format!(
                    "skill '{}' is a builtin skill and has no on-disk files; \
                     omit file_path to read its embedded content",
                    name
                )));
            }
            return self.view_sub_file(base_path, fp, name);
        }

        let content = if let Some(ref embedded) = entry.embedded_content {
            embedded.clone()
        } else {
            std::fs::read_to_string(&entry.skill_file).map_err(|source| {
                ToolSourceError::Transport(format!("read skill {}: {}", name, source))
            })?
        };
        let (_, body) = skill::utils::parse_frontmatter(&content);
        let mut out = body;

        if entry.embedded_content.is_none()
            && entry
                .skill_file
                .file_name()
                .map(|f| f == "SKILL.md")
                .unwrap_or(false)
        {
            if let Ok(rd) = std::fs::read_dir(base_path) {
                let subdirs: Vec<String> = rd
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                    .filter(|n| {
                        ["references", "templates", "scripts", "assets"]
                            .iter()
                            .any(|&d| n == d)
                    })
                    .collect();

                if !subdirs.is_empty() {
                    out.push_str("\n\n## Supporting directories\n");
                    for sd in &subdirs {
                        if let Ok(files) = std::fs::read_dir(base_path.join(sd)) {
                            let names: Vec<String> = files
                                .flatten()
                                .filter(|f| f.path().is_file())
                                .filter_map(|f| f.file_name().to_str().map(|s| s.to_string()))
                                .collect();
                            if !names.is_empty() {
                                out.push_str(&format!("- {}/: {}\n", sd, names.join(", ")));
                            }
                        }
                    }
                    out.push_str("Use file_path parameter to read individual files.\n");
                }
            }
        }

        let content = skill::substitute_template_vars(&out, base_path, None);

        let readiness_header = match entry.metadata.readiness_status() {
            ReadinessStatus::SetupNeeded(missing) => {
                format!(
                    "⚠️ readiness: setup_needed — missing env vars: {}\n\n",
                    missing.join(", ")
                )
            }
            ReadinessStatus::Unsupported(reason) => {
                format!("⚠️ readiness: unsupported — {}\n\n", reason)
            }
            ReadinessStatus::Available => String::new(),
        };

        if let Some(ref store) = self.ctx.usage_store {
            // Hermes `skill_usage.py:408-422`: viewing a skill bumps
            // `view_count` only. `bump_use` must happen at the actual
            // skill-into-prompt injection site, NOT here. The previous
            // double-bump defeated the dual-counter design (view counts
            // for skill-discovery, use counts for skill-actually-applied).
            store.bump_view(name);
        }

        Ok(ToolCallContent::text(format!(
            "<skill_content name=\"{}\">\n{}{}\n</skill_content>",
            name, readiness_header, content
        )))
    }

    fn view_sub_file(
        &self,
        skill_dir: &std::path::Path,
        file_path: &str,
        skill_name: &str,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let target = skill_dir.join(file_path);

        let canonical_skill = skill_dir
            .canonicalize()
            .map_err(|e| ToolSourceError::InvalidInput(format!("invalid skill dir: {}", e)))?;
        let canonical_target = target.canonicalize().map_err(|e| {
            ToolSourceError::InvalidInput(format!(
                "file '{}' not found in skill '{}': {}",
                file_path, skill_name, e
            ))
        })?;

        if !canonical_target.starts_with(&canonical_skill) {
            return Err(ToolSourceError::InvalidInput(format!(
                "path traversal: '{}' is outside skill directory",
                file_path
            )));
        }

        if canonical_target.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "'{}' is a directory, not a file",
                file_path
            )));
        }

        let content = std::fs::read_to_string(&canonical_target)
            .map_err(|e| ToolSourceError::Transport(format!("read {}: {}", file_path, e)))?;

        if let Some(ref store) = self.ctx.usage_store {
            // See note above: view bumps only, use bumps at injection site.
            store.bump_view(skill_name);
        }

        Ok(ToolCallContent::text(format!(
            "<skill_content name=\"{}\" file=\"{}\">\n{}\n</skill_content>",
            skill_name, file_path, content
        )))
    }

    fn view_from_directory(
        &self,
        name: &str,
        file_path: Option<&str>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let skills_dir = self
            .ctx
            .skills_dir()
            .ok_or_else(|| ToolSourceError::InvalidInput("no working folder".to_string()))?;
        if !skills_dir.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "skills directory not found: {}",
                skills_dir.display()
            )));
        }

        let mut temp_registry = SkillRegistry::empty();
        temp_registry.skills =
            skill::discovery::scan_skills_dir_recursive(&skills_dir, skill::SkillSource::Project);

        self.view_from_registry(&temp_registry, name, file_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skill::discovery::{SkillEntry, SkillRegistry, SkillSource};

    fn builtin_registry() -> Arc<SkillRegistry> {
        let mut registry = SkillRegistry::empty();
        registry.add_builtin(
            "workflow",
            "Lua DSL reference for writing multi-agent workflows",
            "---\nname: workflow\ndescription: Lua DSL reference\n---\n\n\
             # Workflow DSL Reference\n\nThis is the builtin workflow skill body.\n",
            vec!["workflow".to_string(), "multi-agent".to_string()],
            vec!["workflow_start".to_string()],
            vec![],
        );
        Arc::new(registry)
    }

    fn make_view_tool(registry: Arc<SkillRegistry>) -> SkillViewTool {
        SkillViewTool::new(Arc::new(SkillContext::from_registry(registry)))
    }

    fn extract_text(content: ToolCallContent) -> String {
        match content {
            ToolCallContent::Text(t) => t,
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn view_builtin_skill_returns_embedded_content() {
        let tool = make_view_tool(builtin_registry());

        let result = tool
            .call(serde_json::json!({"name": "workflow"}), None)
            .await
            .expect("view builtin skill should not fail");

        let body = extract_text(result);

        eprintln!(
            "\n=== skill_view(name=workflow) returned {} chars ===\n{}{}\n=== end ===",
            body.len(),
            &body.chars().take(200).collect::<String>(),
            if body.len() > 200 { "..." } else { "" },
        );

        assert!(
            body.contains("workflow"),
            "body should reference the skill name: {}",
            body
        );
        assert!(
            body.contains("# Workflow DSL Reference"),
            "body should contain embedded markdown heading: {}",
            body
        );
        assert!(
            body.contains("This is the builtin workflow skill body"),
            "body should contain embedded body content: {}",
            body
        );
        assert!(
            !body.contains("name: workflow\n"),
            "body should not include raw frontmatter: {}",
            body
        );
        assert!(
            body.contains("<skill_content"),
            "body should be wrapped in <skill_content> tags: {}",
            body
        );
    }

    #[tokio::test]
    async fn view_builtin_skill_with_file_path_errors() {
        let tool = make_view_tool(builtin_registry());

        let err = tool
            .call(
                serde_json::json!({"name": "workflow", "file_path": "references/foo.md"}),
                None,
            )
            .await
            .expect_err("file_path on builtin skill should error");

        let msg = format!("{}", err);
        assert!(
            msg.contains("builtin"),
            "error should mention builtin: {}",
            msg
        );
        assert!(
            msg.contains("workflow"),
            "error should mention skill name: {}",
            msg
        );
    }

    #[tokio::test]
    async fn view_filesystem_skill_with_file_path_works() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill_dir = tmp.path().join("disk-skill");
        let refs_dir = skill_dir.join("references");
        std::fs::create_dir_all(&refs_dir).expect("mkdir references");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: disk-skill\ndescription: disk version\n---\n\n# Disk Skill\n",
        )
        .expect("write SKILL.md");
        std::fs::write(
            refs_dir.join("api.md"),
            "# API Reference\n\nEndpoint: POST /v1/foo\n",
        )
        .expect("write references/api.md");

        let mut registry = SkillRegistry::empty();
        registry.skills.push(SkillEntry {
            metadata: skill::utils::SkillMetadata {
                name: "disk-skill".to_string(),
                description: "disk version".to_string(),
                ..Default::default()
            },
            base_path: skill_dir.clone(),
            skill_file: skill_dir.join("SKILL.md"),
            source: SkillSource::Project,
            embedded_content: None,
            embedded_files: None,
        });

        let tool = make_view_tool(Arc::new(registry));
        let result = tool
            .call(
                serde_json::json!({"name": "disk-skill", "file_path": "references/api.md"}),
                None,
            )
            .await
            .expect("filesystem skill + file_path should still succeed");

        let body = extract_text(result);
        assert!(
            body.contains("Endpoint: POST /v1/foo"),
            "filesystem sub-file content should be returned: {}",
            body
        );
        assert!(
            body.contains("references/api.md"),
            "file attribute should appear in output: {}",
            body
        );
    }

    #[tokio::test]
    async fn view_filesystem_skill_rejects_path_traversal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill_dir = tmp.path().join("disk-skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: disk-skill\n---\n\nbody\n",
        )
        .expect("write SKILL.md");
        let secret = tmp.path().join("secret.txt");
        std::fs::write(&secret, "top secret\n").expect("write secret");

        let mut registry = SkillRegistry::empty();
        registry.skills.push(SkillEntry {
            metadata: skill::utils::SkillMetadata {
                name: "disk-skill".to_string(),
                description: String::new(),
                ..Default::default()
            },
            base_path: skill_dir.clone(),
            skill_file: skill_dir.join("SKILL.md"),
            source: SkillSource::Project,
            embedded_content: None,
            embedded_files: None,
        });

        let tool = make_view_tool(Arc::new(registry));
        let err = tool
            .call(
                serde_json::json!({"name": "disk-skill", "file_path": "../secret.txt"}),
                None,
            )
            .await
            .expect_err("path traversal should be blocked");

        let msg = format!("{}", err);
        assert!(
            msg.contains("path traversal") || msg.contains("not found") || msg.contains("outside"),
            "error should mention traversal/block: {}",
            msg
        );
    }
}
