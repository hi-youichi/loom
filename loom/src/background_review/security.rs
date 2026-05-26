use super::skill_registry::SkillContent;

const MAX_SKILL_BODY_SIZE: usize = 32 * 1024;
const MAX_SKILL_NAME_LEN: usize = 128;
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
    "DAN mode",
    "developer mode",
];

#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub warnings: Vec<ValidationWarning>,
}

pub fn validate_skill_create(skill: &SkillContent) -> ValidationResult {
    let mut warnings = Vec::new();
    let mut valid = true;

    if skill.name.is_empty() {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Skill name cannot be empty".to_string(),
        });
        valid = false;
    }

    if skill.name.len() > MAX_SKILL_NAME_LEN {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: format!("Skill name too long ({} > {})", skill.name.len(), MAX_SKILL_NAME_LEN),
        });
        valid = false;
    }

    if skill.name.contains("..") || skill.name.contains('/') || skill.name.contains('\\') {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: "Skill name contains path traversal characters".to_string(),
        });
        valid = false;
    }

    if skill.description.len() > MAX_SKILL_DESCRIPTION_LEN {
        warnings.push(ValidationWarning {
            severity: Severity::Warning,
            message: format!("Description very long ({} chars)", skill.description.len()),
        });
    }

    if skill.body.len() > MAX_SKILL_BODY_SIZE {
        warnings.push(ValidationWarning {
            severity: Severity::Critical,
            message: format!("Skill body exceeds {} bytes", MAX_SKILL_BODY_SIZE),
        });
        valid = false;
    }

    for pattern in DANGEROUS_PATTERNS {
        if skill.body.to_lowercase().contains(pattern) {
            warnings.push(ValidationWarning {
                severity: Severity::Critical,
                message: format!("Dangerous pattern detected: {}", pattern),
            });
            valid = false;
        }
    }

    for pattern in INJECTION_PATTERNS {
        if skill.body.to_lowercase().contains(pattern) {
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
            message: format!("Content exceeds {} chars, will be truncated", max_chars),
        });
    }

    for pattern in INJECTION_PATTERNS {
        if content.to_lowercase().contains(pattern) {
            warnings.push(ValidationWarning {
                severity: Severity::Info,
                message: format!("Content contains pattern: {}", pattern),
            });
        }
    }

    ValidationResult { valid, warnings }
}
