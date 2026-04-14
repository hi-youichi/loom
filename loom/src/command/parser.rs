//! Slash command parser: user text -> Option<Command>.

use crate::command::command::Command;

pub fn parse(text: &str) -> Option<Command> {
    let trimmed = text.trim();
    let token = trimmed.split_whitespace().next()?;
    match token {
        "/reset" | "/clear" | "/new" => Some(Command::ResetContext),
        "/compact" => {
            let instructions = trimmed
                .strip_prefix("/compact")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            Some(Command::Compact { instructions })
        }
        "/summarize" => Some(Command::Summarize),
        "/models" => {
            let rest = trimmed
                .strip_prefix("/models")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            match rest {
                Some(q) if q.starts_with("use ") => Some(Command::ModelsUse {
                    model_id: q[4..].trim().to_string(),
                }),
                Some(q) => Some(Command::Models {
                    query: Some(q.to_string()),
                }),
                None => Some(Command::Models { query: None }),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reset_aliases() {
        assert_eq!(parse("/reset"), Some(Command::ResetContext));
        assert_eq!(parse("/clear"), Some(Command::ResetContext));
        assert_eq!(parse("/new"), Some(Command::ResetContext));
        assert_eq!(parse("  /reset  "), Some(Command::ResetContext));
    }

    #[test]
    fn parse_compact_with_and_without_instructions() {
        assert_eq!(
            parse("/compact"),
            Some(Command::Compact { instructions: None })
        );
        assert_eq!(
            parse("/compact focus on auth module"),
            Some(Command::Compact {
                instructions: Some("focus on auth module".into())
            })
        );
    }

    #[test]
    fn parse_summarize() {
        assert_eq!(parse("/summarize"), Some(Command::Summarize));
    }

    #[test]
    fn parse_models_variants() {
        assert_eq!(parse("/models"), Some(Command::Models { query: None }));
        assert_eq!(
            parse("/models gpt"),
            Some(Command::Models {
                query: Some("gpt".into())
            })
        );
        assert_eq!(
            parse("/models use gpt-4o"),
            Some(Command::ModelsUse {
                model_id: "gpt-4o".into()
            })
        );
    }

    #[test]
    fn parse_non_command_returns_none() {
        assert_eq!(parse("hello world"), None);
        assert_eq!(parse("/unknown"), None);
        assert_eq!(parse(""), None);
    }
}
