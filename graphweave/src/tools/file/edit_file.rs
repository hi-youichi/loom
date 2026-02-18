//! Edit-file tool: performs exact string replacements in a file under the working folder.
//!
//! Implements multiple matching strategies in priority order to robustly find and replace
//! text even when indentation, whitespace, or escape sequences differ slightly from the
//! actual file content. Ported from the opencode edit tool (TypeScript).
//!
//! Strategies (tried in order):
//! 1. [`simple_replacer`] – exact substring match
//! 2. [`line_trimmed_replacer`] – per-line trim comparison
//! 3. [`block_anchor_replacer`] – first/last line anchors + Levenshtein similarity
//! 4. [`whitespace_normalized_replacer`] – collapse all whitespace runs to single space
//! 5. [`indentation_flexible_replacer`] – strip common leading indentation
//! 6. [`escape_normalized_replacer`] – unescape `\n`, `\t`, `\\`, etc.
//! 7. [`trimmed_boundary_replacer`] – trim leading/trailing whitespace from `oldString`
//! 8. [`context_aware_replacer`] – anchor on first/last line; ≥50% middle-line match
//! 9. [`multi_occurrence_replacer`] – yields all exact matches (enables `replaceAll`)

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

use super::path::resolve_path_under;

/// Tool name for editing a file.
pub const TOOL_EDIT_FILE: &str = "edit";

const DESCRIPTION: &str = "\
Performs exact string replacements in files.

Usage:
- You must use your `read` tool at least once in the conversation before editing. \
This tool will error if you attempt an edit without reading the file.
- When editing text from read tool output, ensure you preserve the exact indentation \
(tabs/spaces) as it appears AFTER the line number prefix.
- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless \
explicitly required.
- Only use emojis if the user explicitly requests it.
- The edit will FAIL if `oldString` is not found in the file.
- The edit will FAIL if `oldString` is found multiple times and you have not set \
`replaceAll`. Either provide more surrounding context in `oldString` to uniquely identify \
the match, or set `replaceAll` to true.
- Use `replaceAll` for renaming a variable or string across the entire file.";

/// Tool that performs exact string replacements in a file under the working folder.
///
/// Tries multiple matching strategies in priority order so that minor whitespace,
/// indentation, or escape-sequence differences between the LLM's proposed `oldString`
/// and the actual file do not block the edit.
pub struct EditFileTool {
    pub(crate) working_folder: Arc<std::path::PathBuf>,
}

impl EditFileTool {
    /// Creates a new EditFileTool with the given working folder.
    pub fn new(working_folder: Arc<std::path::PathBuf>) -> Self {
        Self { working_folder }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        TOOL_EDIT_FILE
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_EDIT_FILE.to_string(),
            description: Some(DESCRIPTION.to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working folder."
                    },
                    "oldString": {
                        "type": "string",
                        "description": "The text to replace."
                    },
                    "newString": {
                        "type": "string",
                        "description": "The text to replace it with (must differ from oldString)."
                    },
                    "replaceAll": {
                        "type": "boolean",
                        "description": "Replace all occurrences of oldString (default false).",
                        "default": false
                    }
                },
                "required": ["path", "oldString", "newString"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let path_param = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing path".to_string()))?;
        let old_string = args
            .get("oldString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing oldString".to_string()))?;
        let new_string = args
            .get("newString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing newString".to_string()))?;
        let replace_all = args
            .get("replaceAll")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if old_string == new_string {
            return Err(ToolSourceError::InvalidInput(
                "oldString and newString must be different".to_string(),
            ));
        }

        let path = resolve_path_under(self.working_folder.as_ref(), path_param)?;

        // Create / overwrite the file when oldString is empty (new file semantics).
        if old_string.is_empty() {
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        ToolSourceError::Transport(format!("failed to create parent dir: {}", e))
                    })?;
                }
            }
            std::fs::write(&path, new_string).map_err(|e| {
                ToolSourceError::Transport(format!("failed to write file: {}", e))
            })?;
            return Ok(ToolCallContent {
                text: "Edit applied successfully.".to_string(),
            });
        }

        if !path.exists() {
            return Err(ToolSourceError::InvalidInput(format!(
                "file not found: {}",
                path.display()
            )));
        }
        if path.is_dir() {
            return Err(ToolSourceError::InvalidInput(format!(
                "path is a directory, not a file: {}",
                path.display()
            )));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolSourceError::Transport(format!("failed to read file: {}", e)))?;

        let new_content = replace(&content, old_string, new_string, replace_all)
            .map_err(ToolSourceError::InvalidInput)?;

        std::fs::write(&path, &new_content)
            .map_err(|e| ToolSourceError::Transport(format!("failed to write file: {}", e)))?;

        Ok(ToolCallContent {
            text: "Edit applied successfully.".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Levenshtein distance
// ---------------------------------------------------------------------------

fn levenshtein(a: &str, b: &str) -> usize {
    if a.is_empty() || b.is_empty() {
        return a.len().max(b.len());
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    let mut matrix = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        matrix[i][0] = i;
    }
    for j in 0..=n {
        matrix[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }
    matrix[m][n]
}

// ---------------------------------------------------------------------------
// Replacers – each returns the substrings of `content` that match `find`
// ---------------------------------------------------------------------------

/// Returns `find` verbatim, enabling a simple `content.contains(find)` check.
fn simple_replacer(_content: &str, find: &str) -> Vec<String> {
    vec![find.to_string()]
}

/// Matches blocks where every line matches after trimming leading/trailing whitespace.
fn line_trimmed_replacer(content: &str, find: &str) -> Vec<String> {
    let orig: Vec<&str> = content.split('\n').collect();
    let mut search: Vec<&str> = find.split('\n').collect();
    if search.last() == Some(&"") {
        search.pop();
    }
    if search.is_empty() || search.len() > orig.len() {
        return vec![];
    }

    let mut results = Vec::new();
    for i in 0..=orig.len() - search.len() {
        let matches = (0..search.len()).all(|j| orig[i + j].trim() == search[j].trim());
        if matches {
            let start = orig[..i].iter().map(|l| l.len() + 1).sum::<usize>();
            let end = start
                + orig[i..i + search.len()]
                    .iter()
                    .enumerate()
                    .map(|(k, l)| l.len() + if k < search.len() - 1 { 1 } else { 0 })
                    .sum::<usize>();
            results.push(content[start..end].to_string());
        }
    }
    results
}

const SINGLE_CANDIDATE_SIMILARITY_THRESHOLD: f64 = 0.0;
const MULTIPLE_CANDIDATES_SIMILARITY_THRESHOLD: f64 = 0.3;

/// Matches a block by its first and last lines as anchors, using Levenshtein
/// similarity on middle lines to pick the best (or only) candidate.
fn block_anchor_replacer(content: &str, find: &str) -> Vec<String> {
    let orig: Vec<&str> = content.split('\n').collect();
    let mut search: Vec<&str> = find.split('\n').collect();
    if search.len() < 3 {
        return vec![];
    }
    if search.last() == Some(&"") {
        search.pop();
    }
    if search.len() < 3 {
        return vec![];
    }

    let first = search[0].trim();
    let last = search[search.len() - 1].trim();
    let search_size = search.len();

    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for i in 0..orig.len() {
        if orig[i].trim() != first {
            continue;
        }
        for j in (i + 2)..orig.len() {
            if orig[j].trim() == last {
                candidates.push((i, j));
                break;
            }
        }
    }

    if candidates.is_empty() {
        return vec![];
    }

    let extract = |start: usize, end: usize| -> String {
        let s: usize = orig[..start].iter().map(|l| l.len() + 1).sum();
        let e = s
            + orig[start..=end]
                .iter()
                .enumerate()
                .map(|(k, l)| l.len() + if k < end - start { 1 } else { 0 })
                .sum::<usize>();
        content[s..e].to_string()
    };

    let similarity_of = |start: usize, end: usize| -> f64 {
        let actual_size = end - start + 1;
        let lines_to_check = (search_size - 2).min(actual_size - 2);
        if lines_to_check == 0 {
            return 1.0;
        }
        let mut sim = 0.0f64;
        for j in 1..(search_size - 1).min(actual_size - 1) {
            let a = orig[start + j].trim();
            let b = search[j].trim();
            let max_len = a.len().max(b.len());
            if max_len == 0 {
                continue;
            }
            sim += 1.0 - levenshtein(a, b) as f64 / max_len as f64;
        }
        sim / lines_to_check as f64
    };

    if candidates.len() == 1 {
        let (s, e) = candidates[0];
        let mut sim = similarity_of(s, e);
        // Early-exit optimisation mirrors the TS generator break
        if sim < SINGLE_CANDIDATE_SIMILARITY_THRESHOLD {
            // Recompute without early exit to stay faithful to TS logic
            sim = similarity_of(s, e);
        }
        if sim >= SINGLE_CANDIDATE_SIMILARITY_THRESHOLD {
            return vec![extract(s, e)];
        }
        return vec![];
    }

    let mut best: Option<(usize, usize)> = None;
    let mut max_sim = -1.0f64;
    for (s, e) in &candidates {
        let sim = similarity_of(*s, *e);
        if sim > max_sim {
            max_sim = sim;
            best = Some((*s, *e));
        }
    }
    if max_sim >= MULTIPLE_CANDIDATES_SIMILARITY_THRESHOLD {
        if let Some((s, e)) = best {
            return vec![extract(s, e)];
        }
    }
    vec![]
}

/// Collapses all whitespace runs to a single space before comparison.
fn whitespace_normalized_replacer(content: &str, find: &str) -> Vec<String> {
    let normalize = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };
    let norm_find = normalize(find);
    let lines: Vec<&str> = content.split('\n').collect();
    let mut results = Vec::new();

    for line in &lines {
        if normalize(line) == norm_find {
            results.push(line.to_string());
        } else if normalize(line).contains(&norm_find) {
            let words: Vec<&str> = find.trim().split_whitespace().collect();
            if !words.is_empty() {
                let pattern = words
                    .iter()
                    .map(|w| regex::escape(w))
                    .collect::<Vec<_>>()
                    .join(r"\s+");
                if let Ok(re) = regex::Regex::new(&pattern) {
                    if let Some(m) = re.find(line) {
                        results.push(m.as_str().to_string());
                    }
                }
            }
        }
    }

    let find_lines: Vec<&str> = find.split('\n').collect();
    if find_lines.len() > 1 {
        for i in 0..=lines.len().saturating_sub(find_lines.len()) {
            let block = lines[i..i + find_lines.len()].join("\n");
            if normalize(&block) == norm_find {
                results.push(block);
            }
        }
    }
    results
}

/// Strips the common leading indentation from both sides before comparing.
fn indentation_flexible_replacer(content: &str, find: &str) -> Vec<String> {
    let remove_indent = |text: &str| -> String {
        let ls: Vec<&str> = text.split('\n').collect();
        let non_empty: Vec<&str> = ls.iter().filter(|l| !l.trim().is_empty()).copied().collect();
        if non_empty.is_empty() {
            return text.to_string();
        }
        let min = non_empty
            .iter()
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        ls.iter()
            .map(|l| {
                if l.trim().is_empty() {
                    l.to_string()
                } else {
                    l[min.min(l.len())..].to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let norm_find = remove_indent(find);
    let content_lines: Vec<&str> = content.split('\n').collect();
    let find_lines: Vec<&str> = find.split('\n').collect();
    let mut results = Vec::new();

    for i in 0..=content_lines.len().saturating_sub(find_lines.len()) {
        let block = content_lines[i..i + find_lines.len()].join("\n");
        if remove_indent(&block) == norm_find {
            results.push(block);
        }
    }
    results
}

fn unescape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('`') => out.push('`'),
            Some('\\') => out.push('\\'),
            Some('$') => out.push('$'),
            Some('\n') => out.push('\n'),
            Some(c) => {
                out.push('\\');
                out.push(c);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Unescapes `\n`, `\t`, `\\`, etc. in `find` before searching.
fn escape_normalized_replacer(content: &str, find: &str) -> Vec<String> {
    let unescaped = unescape_string(find);
    let mut results = Vec::new();

    if content.contains(&unescaped) {
        results.push(unescaped.clone());
    }

    let lines: Vec<&str> = content.split('\n').collect();
    let find_lines: Vec<&str> = unescaped.split('\n').collect();
    for i in 0..=lines.len().saturating_sub(find_lines.len()) {
        let block = lines[i..i + find_lines.len()].join("\n");
        if unescape_string(&block) == unescaped {
            results.push(block);
        }
    }
    results
}

/// Trims leading/trailing whitespace from `find` before searching.
fn trimmed_boundary_replacer(content: &str, find: &str) -> Vec<String> {
    let trimmed = find.trim();
    if trimmed == find {
        return vec![];
    }
    let mut results = Vec::new();
    if content.contains(trimmed) {
        results.push(trimmed.to_string());
    }
    let lines: Vec<&str> = content.split('\n').collect();
    let find_lines: Vec<&str> = find.split('\n').collect();
    for i in 0..=lines.len().saturating_sub(find_lines.len()) {
        let block = lines[i..i + find_lines.len()].join("\n");
        if block.trim() == trimmed {
            results.push(block);
        }
    }
    results
}

/// Anchors on first and last line; accepts block when ≥50% of middle lines match.
fn context_aware_replacer(content: &str, find: &str) -> Vec<String> {
    let mut find_lines: Vec<&str> = find.split('\n').collect();
    if find_lines.len() < 3 {
        return vec![];
    }
    if find_lines.last() == Some(&"") {
        find_lines.pop();
    }
    if find_lines.len() < 3 {
        return vec![];
    }

    let first = find_lines[0].trim();
    let last = find_lines[find_lines.len() - 1].trim();
    let content_lines: Vec<&str> = content.split('\n').collect();
    let mut results = Vec::new();

    'outer: for i in 0..content_lines.len() {
        if content_lines[i].trim() != first {
            continue;
        }
        for j in (i + 2)..content_lines.len() {
            if content_lines[j].trim() != last {
                continue;
            }
            let block_lines = &content_lines[i..=j];
            if block_lines.len() == find_lines.len() {
                let mut matching = 0usize;
                let mut total = 0usize;
                for k in 1..block_lines.len() - 1 {
                    let bl = block_lines[k].trim();
                    let fl = find_lines[k].trim();
                    if !bl.is_empty() || !fl.is_empty() {
                        total += 1;
                        if bl == fl {
                            matching += 1;
                        }
                    }
                }
                if total == 0 || matching as f64 / total as f64 >= 0.5 {
                    results.push(block_lines.join("\n"));
                }
            }
            break 'outer;
        }
    }
    results
}

/// Yields every exact occurrence of `find`; used to enable `replaceAll`.
fn multi_occurrence_replacer(content: &str, find: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut start = 0;
    while let Some(idx) = content[start..].find(find) {
        results.push(find.to_string());
        start += idx + find.len();
    }
    results
}

// ---------------------------------------------------------------------------
// Public replace entry-point
// ---------------------------------------------------------------------------

/// Replaces `old_string` with `new_string` in `content`, trying each matching
/// strategy in priority order.
///
/// When `replace_all` is false the replacement only succeeds if exactly one
/// occurrence of the matched search string is present; otherwise the next
/// strategy is tried.
///
/// # Errors
///
/// - `"oldString not found in content"` – no strategy produced a match.
/// - `"Found multiple matches …"` – a strategy matched but the string appeared
///   more than once and `replace_all` was false.
pub fn replace(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<String, String> {
    if old_string == new_string {
        return Err("oldString and newString must be different".to_string());
    }

    let replacers: &[fn(&str, &str) -> Vec<String>] = &[
        simple_replacer,
        line_trimmed_replacer,
        block_anchor_replacer,
        whitespace_normalized_replacer,
        indentation_flexible_replacer,
        escape_normalized_replacer,
        trimmed_boundary_replacer,
        context_aware_replacer,
        multi_occurrence_replacer,
    ];

    let mut not_found = true;

    for replacer in replacers {
        for search in replacer(content, old_string) {
            let Some(index) = content.find(&search) else {
                continue;
            };
            not_found = false;

            if replace_all {
                return Ok(content.replace(&search, new_string));
            }

            // Reject if the search string appears more than once.
            let last_index = content.rfind(&search).unwrap();
            if index != last_index {
                continue;
            }

            let mut result = content[..index].to_string();
            result.push_str(new_string);
            result.push_str(&content[index + search.len()..]);
            return Ok(result);
        }
    }

    if not_found {
        Err("oldString not found in content".to_string())
    } else {
        Err("Found multiple matches for oldString. Provide more surrounding lines in \
             oldString to identify the correct match."
            .to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- replace() integration (exercises the full strategy chain) ---

    #[test]
    fn replace_exact_match() {
        let c = "fn foo() {}\nfn bar() {}\n";
        let r = replace(c, "fn foo() {}", "fn baz() {}", false).unwrap();
        assert_eq!(r, "fn baz() {}\nfn bar() {}\n");
    }

    #[test]
    fn replace_not_found_returns_error() {
        let c = "hello world";
        let err = replace(c, "missing", "x", false).unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn replace_multiple_exact_falls_through_to_error() {
        let c = "a b a";
        let err = replace(c, "a", "z", false).unwrap_err();
        assert!(err.contains("multiple"));
    }

    #[test]
    fn replace_all_replaces_every_occurrence() {
        let c = "a b a";
        let r = replace(c, "a", "z", true).unwrap();
        assert_eq!(r, "z b z");
    }

    #[test]
    fn replace_same_old_new_returns_error() {
        let err = replace("x", "x", "x", false).unwrap_err();
        assert!(err.contains("different"));
    }

    // --- simple_replacer ---

    #[test]
    fn simple_replacer_yields_find() {
        let r = simple_replacer("hello world", "hello");
        assert_eq!(r, vec!["hello"]);
    }

    // --- line_trimmed_replacer ---

    #[test]
    fn replace_line_trimmed_single_line() {
        let c = "    fn foo() {}\n    fn bar() {}\n";
        let r = replace(c, "fn foo() {}", "fn baz() {}", false).unwrap();
        assert_eq!(r, "    fn baz() {}\n    fn bar() {}\n");
    }

    #[test]
    fn line_trimmed_replacer_multi_line() {
        let c = "  a\n  b\n  c\n";
        let matches = line_trimmed_replacer(c, "a\nb\nc");
        assert_eq!(matches, vec!["  a\n  b\n  c"]);
    }

    #[test]
    fn line_trimmed_replacer_no_match() {
        let c = "  a\n  b\n";
        let matches = line_trimmed_replacer(c, "x\ny");
        assert!(matches.is_empty());
    }

    #[test]
    fn line_trimmed_replacer_trailing_newline_in_find_is_ignored() {
        let c = "  foo\n";
        let matches = line_trimmed_replacer(c, "foo\n");
        assert_eq!(matches, vec!["  foo"]);
    }

    // --- levenshtein ---

    #[test]
    fn levenshtein_identical_strings() {
        assert_eq!(levenshtein("abc", "abc"), 0);
    }

    #[test]
    fn levenshtein_empty_strings() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", ""), 0);
    }

    #[test]
    fn levenshtein_single_substitution() {
        assert_eq!(levenshtein("kitten", "sitten"), 1);
    }

    #[test]
    fn levenshtein_classic_example() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }

    // --- block_anchor_replacer ---

    #[test]
    fn block_anchor_replacer_single_candidate_accepted() {
        let c = "fn foo() {\n    let x = 1;\n    x\n}\n";
        let find = "fn foo() {\n    let x = 1;\n    x\n}";
        let matches = block_anchor_replacer(c, find);
        assert!(!matches.is_empty(), "should match via block anchor");
        assert!(matches[0].starts_with("fn foo()"));
    }

    #[test]
    fn block_anchor_replacer_too_short_returns_empty() {
        // Less than 3 lines – strategy should skip.
        let c = "fn foo() {}\n";
        let matches = block_anchor_replacer(c, "fn foo() {}");
        assert!(matches.is_empty());
    }

    #[test]
    fn block_anchor_replacer_no_anchor_match_returns_empty() {
        let c = "fn foo() {\n    x\n}\n";
        let find = "fn bar() {\n    x\n}";
        let matches = block_anchor_replacer(c, find);
        assert!(matches.is_empty());
    }

    #[test]
    fn replace_block_anchor_replaces_correct_block() {
        let c = "fn foo() {\n    let x = 1;\n    x\n}\nfn bar() {}\n";
        let find = "fn foo() {\n    let x = 1;\n    x\n}";
        let r = replace(c, find, "fn foo() { 42 }", false).unwrap();
        assert!(r.contains("fn foo() { 42 }"));
        assert!(r.contains("fn bar()"));
    }

    // --- whitespace_normalized_replacer ---

    #[test]
    fn whitespace_normalized_replacer_collapses_spaces() {
        let c = "let   x   =   1;\n";
        let matches = whitespace_normalized_replacer(c, "let x = 1;");
        assert!(!matches.is_empty());
    }

    #[test]
    fn whitespace_normalized_replacer_multi_line() {
        let c = "a  b\nc  d\n";
        let matches = whitespace_normalized_replacer(c, "a b\nc d");
        assert!(!matches.is_empty());
    }

    #[test]
    fn replace_whitespace_normalized() {
        let c = "let   x   =   1;\n";
        let r = replace(c, "let x = 1;", "let x = 99;", false).unwrap();
        assert!(r.contains("99"));
    }

    // --- indentation_flexible_replacer ---

    #[test]
    fn indentation_flexible_replacer_matches_different_indent() {
        let c = "        let x = 1;\n        let y = 2;\n";
        let find = "    let x = 1;\n    let y = 2;";
        let matches = indentation_flexible_replacer(c, find);
        assert!(!matches.is_empty());
    }

    #[test]
    fn replace_indentation_flexible() {
        let c = "        let x = 1;\n        let y = 2;\n";
        let find = "    let x = 1;\n    let y = 2;";
        let r = replace(c, find, "    let x = 99;\n    let y = 2;", false).unwrap();
        assert!(r.contains("99"));
    }

    // --- escape_normalized_replacer ---

    #[test]
    fn unescape_string_newline() {
        assert_eq!(unescape_string("a\\nb"), "a\nb");
    }

    #[test]
    fn unescape_string_tab() {
        assert_eq!(unescape_string("a\\tb"), "a\tb");
    }

    #[test]
    fn unescape_string_backslash() {
        assert_eq!(unescape_string("a\\\\b"), "a\\b");
    }

    #[test]
    fn unescape_string_unknown_escape_preserved() {
        assert_eq!(unescape_string("a\\zb"), "a\\zb");
    }

    #[test]
    fn replace_escape_normalized() {
        let c = "msg = \"hello\\nworld\";\n";
        let r = replace(c, "msg = \"hello\\nworld\";", "msg = \"bye\";", false).unwrap();
        assert_eq!(r, "msg = \"bye\";\n");
    }

    // --- trimmed_boundary_replacer ---

    #[test]
    fn trimmed_boundary_replacer_skips_when_find_already_trimmed() {
        // When find has no surrounding whitespace, trimmed == find → strategy returns empty
        // (the simple/line_trimmed strategies handle this case).
        let c = "hello\n";
        let matches = trimmed_boundary_replacer(c, "hello");
        assert!(matches.is_empty());
    }

    #[test]
    fn trimmed_boundary_replacer_finds_trimmed_version() {
        let c = "hello\n";
        let matches = trimmed_boundary_replacer(c, "  hello  ");
        assert!(!matches.is_empty());
    }

    #[test]
    fn replace_trimmed_boundary() {
        let c = "hello world\n";
        let r = replace(c, "  hello world  ", "goodbye", false).unwrap();
        assert_eq!(r, "goodbye\n");
    }

    // --- context_aware_replacer ---

    #[test]
    fn context_aware_replacer_matches_same_block() {
        let c = "fn foo() {\n    let x = 1;\n    x\n}\n";
        let find = "fn foo() {\n    let x = 1;\n    x\n}";
        let matches = context_aware_replacer(c, find);
        assert!(!matches.is_empty(), "context_aware should match");
    }

    #[test]
    fn context_aware_replacer_too_short_skips() {
        let matches = context_aware_replacer("a\nb\n", "a\nb");
        assert!(matches.is_empty());
    }

    #[test]
    fn context_aware_replacer_low_middle_similarity_rejects() {
        // First and last lines match but all middle lines are completely different.
        let c = "START\na\nb\nc\nd\ne\nEND\n";
        let find = "START\n1\n2\n3\n4\n5\nEND";
        let matches = context_aware_replacer(c, find);
        assert!(
            matches.is_empty(),
            "0% middle match should be rejected (threshold 50%)"
        );
    }

    #[test]
    fn replace_context_aware() {
        let c = "fn foo() {\n    let x = 1;\n    x\n}\n";
        let find = "fn foo() {\n    let x = 1;\n    x\n}";
        let r = replace(c, find, "fn foo() { 0 }", false).unwrap();
        assert!(r.contains("fn foo() { 0 }"));
    }

    // --- multi_occurrence_replacer ---

    #[test]
    fn multi_occurrence_replacer_yields_one_entry_per_match() {
        let c = "ab_ab_ab";
        let v = multi_occurrence_replacer(c, "ab");
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn multi_occurrence_replacer_no_match_returns_empty() {
        let v = multi_occurrence_replacer("hello", "xyz");
        assert!(v.is_empty());
    }

    #[test]
    fn replace_all_via_multi_occurrence() {
        let c = "x = x + x;";
        let r = replace(c, "x", "y", true).unwrap();
        assert_eq!(r, "y = y + y;");
    }
}
