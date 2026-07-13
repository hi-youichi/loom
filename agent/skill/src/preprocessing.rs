//! Skill preprocessing — template variable substitution and inline shell expansion.

use std::path::Path;

static TEMPLATE_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"\$\{(LOOM_SKILL_DIR|HERMES_SKILL_DIR|LOOM_SESSION_ID|HERMES_SESSION_ID)\}")
        .unwrap()
});

static INLINE_SHELL_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"!`([^`\n]+)`").unwrap());

const INLINE_SHELL_MAX_OUTPUT: usize = 4000;

pub fn substitute_template_vars(
    content: &str,
    skill_dir: &Path,
    session_id: Option<&str>,
) -> String {
    let dir_str = skill_dir.to_string_lossy().to_string();
    TEMPLATE_RE
        .replace_all(content, |caps: &regex::Captures| {
            match &caps[1] {
                // Legacy HERMES_* variables are deprecated aliases for LOOM_* equivalents.
                "LOOM_SKILL_DIR" | "HERMES_SKILL_DIR" => dir_str.clone(),
                "LOOM_SESSION_ID" | "HERMES_SESSION_ID" => session_id.unwrap_or("").to_string(),
                _ => caps[0].to_string(),
            }
        })
        .to_string()
}

pub fn expand_inline_shell(content: &str, skill_dir: Option<&Path>) -> String {
    INLINE_SHELL_RE
        .replace_all(content, |caps: &regex::Captures| {
            let cmd = &caps[1];
            let cwd = skill_dir;
            match std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
                .arg(if cfg!(windows) { "/C" } else { "-c" })
                .arg(cmd)
                .current_dir(cwd.unwrap_or(std::path::Path::new(".")))
                .output()
            {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let trimmed = stdout.trim();
                    if trimmed.is_empty() {
                        format!("(no output from: {})", cmd)
                    } else if trimmed.len() > INLINE_SHELL_MAX_OUTPUT {
                        let end = trimmed
                            .char_indices()
                            .take_while(|(i, _)| *i < INLINE_SHELL_MAX_OUTPUT)
                            .last()
                            .map(|(i, c)| i + c.len_utf8())
                            .unwrap_or(INLINE_SHELL_MAX_OUTPUT);
                        format!("{}...", &trimmed[..end])
                    } else {
                        trimmed.to_string()
                    }
                }
                Err(e) => format!("(error: {})", e),
            }
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn substitute_loom_skill_dir() {
        let content = "Path: ${LOOM_SKILL_DIR}/scripts/setup.sh";
        let dir = Path::new("/home/user/.loom/skills/my-skill");
        let result = substitute_template_vars(content, dir, None);
        assert_eq!(
            result,
            "Path: /home/user/.loom/skills/my-skill/scripts/setup.sh"
        );
    }

    #[test]
    fn substitute_legacy_skill_dir_backward_compat() {
        let content = "Path: ${HERMES_SKILL_DIR}/scripts/setup.sh";
        let dir = Path::new("/home/user/.loom/skills/my-skill");
        let result = substitute_template_vars(content, dir, None);
        assert_eq!(
            result,
            "Path: /home/user/.loom/skills/my-skill/scripts/setup.sh"
        );
    }

    #[test]
    fn substitute_session_id() {
        let content = "Session: ${LOOM_SESSION_ID}";
        let dir = Path::new(".");
        let result = substitute_template_vars(content, dir, Some("sess-123"));
        assert_eq!(result, "Session: sess-123");
    }

    #[test]
    fn substitute_legacy_session_id_backward_compat() {
        let content = "Session: ${HERMES_SESSION_ID}";
        let dir = Path::new(".");
        let result = substitute_template_vars(content, dir, Some("sess-456"));
        assert_eq!(result, "Session: sess-456");
    }

    #[test]
    fn substitute_session_id_none_yields_empty() {
        let content = "Session: ${LOOM_SESSION_ID}";
        let dir = Path::new(".");
        let result = substitute_template_vars(content, dir, None);
        assert_eq!(result, "Session: ");
    }

    #[test]
    fn substitute_multiple_vars_in_one_line() {
        let content = "dir=${LOOM_SKILL_DIR} sid=${LOOM_SESSION_ID}";
        let dir = Path::new("/skills/x");
        let result = substitute_template_vars(content, dir, Some("abc"));
        assert_eq!(result, "dir=/skills/x sid=abc");
    }

    #[test]
    fn substitute_no_match_preserves_content() {
        let content = "No variables here, just ${OTHER_VAR} and plain text.";
        let dir = Path::new(".");
        let result = substitute_template_vars(content, dir, None);
        assert_eq!(result, content);
    }

    #[test]
    fn substitute_empty_content() {
        let content = "";
        let dir = Path::new(".");
        let result = substitute_template_vars(content, dir, None);
        assert_eq!(result, "");
    }

    #[test]
    fn expand_inline_shell_echo() {
        let content = "Today is !`echo hello`.";
        let result = expand_inline_shell(content, None);
        assert_eq!(result, "Today is hello.");
    }

    #[test]
    fn expand_inline_shell_multiline_only_first_line_match() {
        let content = "Line1\n!`echo ok`\nLine3";
        let result = expand_inline_shell(content, None);
        assert!(result.contains("ok"));
    }

    #[test]
    fn expand_inline_shell_no_match() {
        let content = "No shell expansion here.";
        let result = expand_inline_shell(content, None);
        assert_eq!(result, "No shell expansion here.");
    }

    #[test]
    fn expand_inline_shell_error_yields_error_message() {
        let content = "Run: !`false_command_that_does_not_exist_12345`.";
        let result = expand_inline_shell(content, None);
        assert!(
            result.contains("error:")
                || result.contains("no output")
                || result.contains("false_command")
        );
    }

    #[test]
    fn expand_inline_shell_empty_output() {
        let content = "Empty: !`echo -n`.";
        let result = expand_inline_shell(content, None);
        assert!(result.contains("no output") || result.contains("Empty:"));
    }
}
