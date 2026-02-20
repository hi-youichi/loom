//! Parse `.env` file into key-value map (no overwrite of existing env here; applied in lib).

use std::collections::HashMap;
use std::path::Path;

/// Paths to try for `.env`: `override_dir` if given, else current directory.
fn dotenv_path(override_dir: Option<&Path>) -> Option<std::path::PathBuf> {
    let dir = override_dir
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())?;
    let path = dir.join(".env");
    if path.exists() && path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Minimal .env parser: lines as KEY=VALUE, skip empty and # comments, trim key and value.
///
/// * Empty value: `KEY=` or `KEY=""` yields key with value `""`.
/// * Comments: only lines starting with `#` (after trim) are skipped; `#` inside value is kept.
/// * Quotes: double-quoted values support `\"` escape; single-quoted values are stripped, no escape.
/// * No multiline or line continuation.
fn parse_dotenv(content: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim().to_string();
        let value = v.trim().to_string();
        // Remove surrounding quotes if present
        let value = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value[1..value.len() - 1].replace("\\\"", "\"")
        } else {
            value
        };
        let value = value
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .map(|s| s.to_string())
            .unwrap_or(value);
        if !key.is_empty() {
            out.insert(key, value);
        }
    }
    out
}

/// Load `.env` from override_dir or current directory into a map. Missing file returns empty map.
pub fn load_env_map(override_dir: Option<&Path>) -> std::io::Result<HashMap<String, String>> {
    let path = match dotenv_path(override_dir) {
        Some(p) => p,
        None => return Ok(HashMap::new()),
    };
    let content = std::fs::read_to_string(&path)?;
    Ok(parse_dotenv(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let s = "FOO=bar\nBAZ=quux\n";
        let m = parse_dotenv(s);
        assert_eq!(m.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(m.get("BAZ"), Some(&"quux".to_string()));
    }

    #[test]
    fn skip_comments_and_empty() {
        let s = "\n# comment\nKEY=val\n  \n";
        let m = parse_dotenv(s);
        assert_eq!(m.get("KEY"), Some(&"val".to_string()));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn quoted_value() {
        let s = r#"KEY="hello world""#;
        let m = parse_dotenv(s);
        assert_eq!(m.get("KEY"), Some(&"hello world".to_string()));
    }

    #[test]
    fn single_quoted_value() {
        let s = "KEY='single quoted'";
        let m = parse_dotenv(s);
        assert_eq!(m.get("KEY"), Some(&"single quoted".to_string()));
    }

    #[test]
    fn line_without_equals_skipped() {
        let s = "NOT_KEY_VALUE\nKEY=val\n";
        let m = parse_dotenv(s);
        assert_eq!(m.get("KEY"), Some(&"val".to_string()));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn empty_key_skipped() {
        let s = "=value_only\nKEY=ok\n";
        let m = parse_dotenv(s);
        assert_eq!(m.get("KEY"), Some(&"ok".to_string()));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn empty_content_returns_empty_map() {
        let m = parse_dotenv("");
        assert!(m.is_empty());
    }

    #[test]
    fn empty_value_key_equals() {
        let s = "KEY=\nOTHER=val\n";
        let m = parse_dotenv(s);
        assert_eq!(m.get("KEY"), Some(&"".to_string()));
        assert_eq!(m.get("OTHER"), Some(&"val".to_string()));
    }

    #[test]
    fn empty_value_double_quotes() {
        let s = r#"KEY="""#;
        let m = parse_dotenv(s);
        assert_eq!(m.get("KEY"), Some(&"".to_string()));
    }

    #[test]
    fn escaped_quote_in_double_quoted() {
        let s = r#"KEY="say \"hi\"""#;
        let m = parse_dotenv(s);
        assert_eq!(m.get("KEY"), Some(&"say \"hi\"".to_string()));
    }

    #[test]
    fn load_env_map_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let m = load_env_map(Some(dir.path())).unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn load_env_map_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        std::fs::write(&env_path, "A=1\nB=2\n").unwrap();
        let m = load_env_map(Some(dir.path())).unwrap();
        assert_eq!(m.get("A"), Some(&"1".to_string()));
        assert_eq!(m.get("B"), Some(&"2".to_string()));
    }
}
