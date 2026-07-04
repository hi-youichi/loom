//! Runtime skill validation — used during create/edit/patch operations.
//!
//! This module provides content-level validation for skill writes. It is
//! distinct from `guard` (which performs file-level installation scanning):
//!
//! - **`validation` (this module)**: runtime, content-based, called by tools
//!   before writing a skill. Inputs are in-memory `SkillContent` / path strings.
//!   Returns `ValidationResult { valid, warnings }` with severity levels.
//!
//! - **`guard`**: install-time, file-based, called when a skill is downloaded
//!   from an external source. Inputs are `&Path` + `TrustLevel`. Returns
//!   `ScanResult { findings, verdict }` with `Verdict::Safe/Warning/Blocked`.

use serde::{Deserialize, Serialize};

use crate::storage::SkillContent;

/// Hermes `MAX_SKILL_BODY_SIZE = 100_000` (chars). We measure in bytes here
/// (UTF-8 worst-case 4 bytes/char) and round up to 100 KiB so multi-byte
/// content is not silently truncated below the Hermes-equivalent character
/// budget. Aligned with `skill_manager_tool.py:164` (tools/skill_manager_tool.py).
pub const MAX_SKILL_BODY_SIZE: usize = 100 * 1024;
const MAX_SKILL_NAME_LEN: usize = 64;
const MAX_SKILL_DESCRIPTION_LEN: usize = 1024;

static DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf",
    "rm -r /",
    ":(){ :|:& };:",
    "del /f /s /q",
    "format c:",
    "shutdown",
    "exec(",
    "eval(",
    "system(",
    "os.system",
    "subprocess.call",
    "child_process.exec",
    "__import__",
    "os.popen",
    "passthru",
    "shell_exec",
];

static INJECTION_PATTERNS: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous",
    "disregard your instructions",
    "forget your instructions",
    "new instructions:",
    "system prompt:",
    "you are now",
    "jailbreak",
    "dan mode",
    "developer mode",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub warnings: Vec<ValidationWarning>,
}

pub fn validate_frontmatter(content: &str) -> Result<(String, String, Vec<String>, String), ValidationResult> {
    let mut warnings = Vec::new();

    if content.trim().is_empty() {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Content cannot be empty.".to_string(),
        });
        return Err(ValidationResult {
            valid: false,
            warnings,
        });
    }
    if !content.starts_with("---") {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "SKILL.md must start with YAML frontmatter (---).".to_string(),
        });
        return Err(ValidationResult {
            valid: false,
            warnings,
        });
    }

    let after_first = &content[3..];
    let close_marker = "\n---\n";
    let end = match after_first.find(close_marker) {
        Some(idx) => idx,
        None => match after_first.find("\n---") {
            Some(idx) => idx,
            None => {
                warnings.push(ValidationWarning {
                    severity: Severity::Critical,
                    message: "SKILL.md frontmatter is not closed.".to_string(),
                });
                return Err(ValidationResult {
                    valid: false,
                    warnings,
                });
            }
        },
    };

    let yaml_str = &after_first[..end];
    let body = if let Some(idx) = after_first.find(close_marker) {
        after_first[idx + close_marker.len()..].to_string()
    } else if let Some(idx) = after_first.find("\n---") {
        after_first[idx + 4..]
            .trim_start_matches(['\n', '\r'])
            .to_string()
    } else {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "SKILL.md frontmatter is not closed.".to_string(),
        });
        return Err(ValidationResult {
            valid: false,
            warnings,
        });
    };

    let parsed: serde_yaml::Value = match serde_yaml::from_str(yaml_str) {
        Ok(v) => v,
        Err(e) => {
            warnings.push(ValidationWarning {
                severity: Severity::Critical,
                message: format!("YAML frontmatter parse error: {}", e),
            });
            return Err(ValidationResult {
                valid: false,
                warnings,
            });
        }
    };

    let mapping = match parsed.as_mapping() {
        Some(m) => m,
        None => {
            warnings.push(ValidationWarning {
                severity: Severity::Critical,
                message: "Frontmatter must be a YAML mapping (key: value pairs).".to_string(),
            });
            return Err(ValidationResult {
                valid: false,
                warnings,
            });
        }
    };

    let name = match mapping.get(serde_yaml::Value::String("name".into())) {
        Some(v) => match v.as_str() {
            Some(s) => s.to_string(),
            None => {
                warnings.push(ValidationWarning {
                    severity: Severity::Critical,
                    message: "Frontmatter 'name' must be a string.".to_string(),
                });
                return Err(ValidationResult {
                    valid: false,
                    warnings,
                });
            }
        },
        None => {
            warnings.push(ValidationWarning {
                severity: Severity::Critical,
                message: "Frontmatter must include 'name' field.".to_string(),
            });
            return Err(ValidationResult {
                valid: false,
                warnings,
            });
        }
    };

    let description = match mapping.get(serde_yaml::Value::String("description".into())) {
        Some(v) => match v.as_str() {
            Some(s) => s.to_string(),
            None => {
                warnings.push(ValidationWarning {
                    severity: Severity::Critical,
                    message: "Frontmatter 'description' must be a string.".to_string(),
                });
                return Err(ValidationResult {
                    valid: false,
                    warnings,
                });
            }
        },
        None => {
            warnings.push(ValidationWarning {
                severity: Severity::Critical,
                message: "Frontmatter must include 'description' field.".to_string(),
            });
            return Err(ValidationResult {
                valid: false,
                warnings,
            });
        }
    };

    if body.trim().is_empty() {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "SKILL.md must have content after the frontmatter (instructions, procedures, etc.)."
                .to_string(),
        });
        return Err(ValidationResult {
            valid: false,
            warnings,
        });
    }

    let triggers: Vec<String> = mapping
        .get(serde_yaml::Value::String("triggers".into()))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok((name, description, triggers, body))
}

pub fn validate_name_match(frontmatter_name: &str, arg_name: &str) -> ValidationResult {
    if frontmatter_name != arg_name {
        ValidationResult {
            valid: false,
            warnings: vec![ValidationWarning {
                severity: Severity::Critical,
                message: format!(
                    "Frontmatter name '{}' does not match args.name '{}'",
                    frontmatter_name, arg_name
                ),
            }],
        }
    } else {
        ValidationResult {
            valid: true,
            warnings: vec![],
        }
    }
}

pub fn validate_skill_name(name: &str) -> ValidationResult {
    let mut warnings = Vec::new();
    let mut valid = true;

    if name.is_empty() {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Skill name cannot be empty".to_string(),
        });
        valid = false;
    }

    if name.len() > MAX_SKILL_NAME_LEN {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: format!(
                "Skill name must be {} characters or fewer (got {})",
                MAX_SKILL_NAME_LEN,
                name.len()
            ),
        });
        valid = false;
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.')
    {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Skill name must be lowercase alphanumeric with hyphens, dots, and underscores."
                .to_string(),
        });
        valid = false;
    }

if name.contains("..") || name.contains('/') || name.contains('\\') {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Skill name contains path traversal characters".to_string(),
        });
        valid = false;
    }

    // First character must be alphanumeric — Hermes `curator.py:109`
    // (`_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")`). Names like
    // `-foo` or `._x` are rejected to keep directory listings sorted sanely
    // and avoid accidental shell-flag interpretation on tooling that passes
    // the name as the first arg.
    if !name.is_empty() && !name.chars().next().unwrap().is_ascii_alphanumeric() {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Skill name must start with an alphanumeric character".to_string(),
        });
        valid = false;
    }

    ValidationResult { valid, warnings }
}

pub fn validate_skill_create(skill: &SkillContent) -> ValidationResult {
    let mut warnings = Vec::new();
    let mut valid = true;

    let name_result = validate_skill_name(&skill.name);
    warnings.extend(name_result.warnings);
    if !name_result.valid {
        valid = false;
    }

    if skill.description.len() > MAX_SKILL_DESCRIPTION_LEN {
        warnings.push(ValidationWarning {
            severity: Severity::Warning,
            message: format!(
                "Description very long ({} chars)",
                skill.description.len()
            ),
        });
    }

    if skill.body.len() > MAX_SKILL_BODY_SIZE {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: format!("Skill body exceeds {} bytes", MAX_SKILL_BODY_SIZE),
        });
        valid = false;
    }

    let body_lower = skill.body.to_lowercase();
    for pattern in DANGEROUS_PATTERNS {
        if body_lower.contains(&pattern.to_lowercase()) {
            warnings.push(ValidationWarning {
                severity: Severity::Critical,
                message: format!("Dangerous pattern detected: {}", pattern),
            });
            valid = false;
        }
    }

    for pattern in INJECTION_PATTERNS {
        if body_lower.contains(&pattern.to_lowercase()) {
            warnings.push(ValidationWarning {
                severity: Severity::Warning,
                message: format!("Possible prompt injection: {}", pattern),
            });
        }
    }

    if skill.body.contains("<script") || skill.body.contains("javascript:") {
        warnings.push(ValidationWarning {
            severity: Severity::Warning,
            message: "Script tag or javascript: URI detected".to_string(),
        });
    }

    ValidationResult { valid, warnings }
}

pub fn validate_skill_path(path: &str) -> ValidationResult {
    let mut warnings = Vec::new();
    let mut valid = true;

    if path.contains("..") {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Path traversal detected".to_string(),
        });
        valid = false;
    }

    if path.starts_with('/') || path.starts_with('\\') {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Absolute path not allowed".to_string(),
        });
        valid = false;
    }

    if path.contains('\\') {
        warnings.push(ValidationWarning {
            severity: Severity::Warning,
            message: "Backslash in path".to_string(),
        });
    }

    ValidationResult { valid, warnings }
}

pub fn validate_memory_content(content: &str, max_chars: usize) -> ValidationResult {
    let mut warnings = Vec::new();
    let valid = true;

    if content.len() > max_chars {
        warnings.push(ValidationWarning {
            severity: Severity::Warning,
            message: format!(
                "Content exceeds {} chars, will be truncated",
                max_chars
            ),
        });
    }

    let content_lower = content.to_lowercase();
    for pattern in INJECTION_PATTERNS {
        if content_lower.contains(&pattern.to_lowercase()) {
            warnings.push(ValidationWarning {
                severity: Severity::Info,
                message: format!("Content contains pattern: {}", pattern),
            });
        }
    }

    ValidationResult { valid, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Lifecycle, Source};

    fn skill(name: &str, description: &str, body: &str) -> SkillContent {
        SkillContent {
            name: name.to_string(),
            description: description.to_string(),
            triggers: vec![],
            lifecycle: Lifecycle::Active,
            source: Source::Auto,
            created_by: None,
            body: body.to_string(),
            raw: body.to_string(),
        }
    }

    // ── validate_skill_create ──

    #[test]
    fn valid_skill_passes() {
        let result = validate_skill_create(&skill("my-skill", "A skill", "normal content"));
        assert!(result.valid);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn empty_name_fails() {
        let result = validate_skill_create(&skill("", "desc", "body"));
        assert!(!result.valid);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("cannot be empty")));
    }

    #[test]
    fn name_too_long_fails() {
        let long_name = "x".repeat(MAX_SKILL_NAME_LEN + 1);
        let result = validate_skill_create(&skill(&long_name, "desc", "body"));
        assert!(!result.valid);
        assert!(result.warnings.iter().any(|w| w.message.contains("characters or fewer")));
    }

    #[test]
    fn name_at_max_length_passes() {
        let name = "x".repeat(MAX_SKILL_NAME_LEN);
        let result = validate_skill_create(&skill(&name, "desc", "body"));
        assert!(!result.warnings.iter().any(|w| w.message.contains("characters or fewer")));
    }

    #[test]
    fn path_traversal_in_name_fails() {
        for bad_name in &["../evil", "foo/bar", "foo\\bar"] {
            let result = validate_skill_create(&skill(bad_name, "desc", "body"));
            assert!(!result.valid, "expected '{}' to fail", bad_name);
            assert!(result
                .warnings
                .iter()
                .any(|w| w.message.contains("path traversal")));
        }
    }

    #[test]
    fn body_too_large_fails() {
        let big_body = "x".repeat(MAX_SKILL_BODY_SIZE + 1);
        let result = validate_skill_create(&skill("ok", "desc", &big_body));
        assert!(!result.valid);
        assert!(result.warnings.iter().any(|w| w.message.contains("exceeds")));
    }

    #[test]
    fn body_at_max_size_passes() {
        let body = "x".repeat(MAX_SKILL_BODY_SIZE);
        let result = validate_skill_create(&skill("ok", "desc", &body));
        assert!(!result.warnings.iter().any(|w| w.message.contains("exceeds")));
    }

    #[test]
    fn description_too_long_warns_but_valid() {
        let long_desc = "d".repeat(MAX_SKILL_DESCRIPTION_LEN + 1);
        let result = validate_skill_create(&skill("ok", &long_desc, "body"));
        assert!(result.valid);
        assert!(result.warnings.iter().any(|w| w.message.contains("very long")));
    }

    #[test]
    fn dangerous_patterns_detected() {
        for pattern in DANGEROUS_PATTERNS {
            let body = format!("some content with {} embedded", pattern);
            let result = validate_skill_create(&skill("ok", "desc", &body));
            assert!(!result.valid, "expected '{}' to be detected", pattern);
        }
    }

    #[test]
    fn dangerous_patterns_case_insensitive() {
        let result = validate_skill_create(&skill("ok", "desc", "RM -RF /"));
        assert!(!result.valid);
    }

    #[test]
    fn injection_patterns_lowercase_detected() {
        for pattern in &[
            "ignore previous instructions",
            "ignore all previous",
            "disregard your instructions",
            "forget your instructions",
            "new instructions:",
            "you are now",
            "jailbreak",
        ] {
            let body = format!("some content with {} embedded", pattern);
            let result = validate_skill_create(&skill("ok", "desc", &body));
            assert!(
                result.valid,
                "injection '{}' should not make invalid",
                pattern
            );
            assert!(
                result.warnings.iter().any(|w| w.message.contains("Possible")
                    || w.message.contains("injection")),
                "pattern '{}' should produce warning",
                pattern
            );
        }
    }

    #[test]
    fn injection_patterns_uppercase_now_detected() {
        let result = validate_skill_create(&skill("ok", "desc", "this is dan mode activated"));
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("injection")));
    }

    #[test]
    fn injection_pattern_developer_mode_detected() {
        let result = validate_skill_create(&skill("ok", "desc", "entering developer mode now"));
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("injection")));
    }

    #[test]
    fn injection_pattern_system_prompt_case_insensitive() {
        let result = validate_skill_create(&skill("ok", "desc", "system prompt: do evil"));
        assert!(result.valid);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("Possible")));
    }

    #[test]
    fn script_tag_warns() {
        let result = validate_skill_create(&skill("ok", "desc", "<script>alert(1)</script>"));
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("Script tag")));
    }

    #[test]
    fn javascript_uri_warns() {
        let result = validate_skill_create(&skill("ok", "desc", "javascript:void(0)"));
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("javascript")));
    }

    #[test]
    fn multiple_issues_stacked() {
        let result = validate_skill_create(&skill("", "desc", "rm -rf /"));
        assert!(!result.valid);
        assert!(result.warnings.len() >= 2);
    }

    // ── validate_skill_path ──

    #[test]
    fn valid_path_passes() {
        let result = validate_skill_path("skills/my-skill.md");
        assert!(result.valid);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn path_traversal_fails() {
        let result = validate_skill_path("../../etc/passwd");
        assert!(!result.valid);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("traversal")));
    }

    #[test]
    fn absolute_path_unix_fails() {
        let result = validate_skill_path("/etc/passwd");
        assert!(!result.valid);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("Absolute")));
    }

    #[test]
    fn absolute_path_windows_fails() {
        let result = validate_skill_path("\\\\server\\share");
        assert!(!result.valid);
    }

    #[test]
    fn backslash_warns() {
        let result = validate_skill_path("path\\to\\skill");
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("Backslash")));
    }

#[test]
    fn simple_name_passes() {
        let result = validate_skill_path("my-skill");
        assert!(result.valid);
    }

    #[test]
    fn first_char_must_be_alphanumeric() {
        for bad in &[".foo", "-foo", "_foo", "..hidden"] {
            let r = validate_skill_name(bad);
            assert!(!r.valid, "expected '{}' to fail first-char check", bad);
            assert!(
                r.warnings
                    .iter()
                    .any(|w| w.message.contains("start with an alphanumeric")),
                "expected first-char warning for '{}'",
                bad
            );
        }
        for good in &["foo", "foo-bar", "1foo", "x.y", "a_b"] {
            let r = validate_skill_name(good);
            assert!(
                !r.warnings
                    .iter()
                    .any(|w| w.message.contains("start with an alphanumeric")),
                "unexpected first-char warning for '{}'",
                good
            );
        }
    }

    // ── validate_memory_content ──

    #[test]
    fn short_content_passes() {
        let result = validate_memory_content("normal content", 1000);
        assert!(result.valid);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn oversized_content_warns_but_valid() {
        let long = "x".repeat(1001);
        let result = validate_memory_content(&long, 1000);
        assert!(result.valid);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.message.contains("truncated")));
    }

    #[test]
    fn memory_injection_pattern_info_level() {
        let result = validate_memory_content("ignore previous instructions", 1000);
        assert!(result.valid);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.severity == Severity::Info));
    }

    #[test]
    fn exactly_at_limit_passes() {
        let content = "x".repeat(1000);
        let result = validate_memory_content(&content, 1000);
        assert!(result.warnings.is_empty());
    }
}
