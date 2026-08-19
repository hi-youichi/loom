//! Text parsers over git CLI output. Parity baseline: the git2 backend (B1)
//! must produce structs identical to these parsers' output.

use crate::types::{
    GitDiffHunk, GitDiffLine, GitDiffLineKind, GitDiffStat, GitDiffSummary, GitFileStatus,
    GitInProgress, GitOperation, GitStatus, GitStatusFile,
};

/// Porcelain v2 quotes non-ASCII paths as C strings: `"\344\270\255..."`.
/// Unquote and decode octal escapes so paths match git2's raw UTF-8.
pub fn unquote_path(field: &str) -> String {
    let trimmed = field.trim();
    if !(trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2) {
        return trimmed.to_string();
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let mut out = Vec::with_capacity(inner.len());
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if i + 3 < bytes.len() + 1 && bytes.len() >= i + 4 {
                    if let Ok(v) = u8::from_str_radix(
                        std::str::from_utf8(&bytes[i + 1..i + 4]).unwrap_or(""),
                        8,
                    ) {
                        out.push(v);
                        i += 4;
                        continue;
                    }
                }
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_porcelain_status_v2(output: &str) -> GitStatus {
    let mut branch = String::new();
    let mut upstream: Option<String> = None;
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut files = Vec::new();
    let mut in_progress: Option<GitInProgress> = None;
    let mut conflict_files = Vec::new();

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            branch = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            for p in parts {
                if let Some(n) = p.strip_prefix('+') {
                    ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = p.strip_prefix('-') {
                    behind = n.parse().unwrap_or(0);
                }
            }
        } else if line.starts_with("u ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let path = unquote_path(parts.last().unwrap_or(&""));
                conflict_files.push(path.clone());
                files.push(GitStatusFile {
                    path,
                    index_status: GitFileStatus::Unmerged,
                    working_status: GitFileStatus::Unmerged,
                });
            }
        } else if line.starts_with("1 ") {
            let tokens: Vec<&str> = line.splitn(9, ' ').collect();
            if tokens.len() < 9 {
                continue;
            }
            let xy = tokens.get(1).unwrap_or(&"..");
            let index_status = parse_xy_status(xy.chars().next().unwrap_or('.'));
            let working_status = parse_xy_status(xy.chars().nth(1).unwrap_or('.'));
            let path_part = tokens.get(8).unwrap_or(&"");

            if matches!(index_status, GitFileStatus::Unmerged)
                || matches!(working_status, GitFileStatus::Unmerged)
            {
                conflict_files.push(unquote_path(path_part));
            }

            files.push(GitStatusFile {
                path: unquote_path(path_part),
                index_status,
                working_status,
            });
        } else if line.starts_with("2 ") {
            let tokens: Vec<&str> = line.splitn(11, ' ').collect();
            if tokens.len() < 11 {
                continue;
            }
            let xy = tokens.get(1).unwrap_or(&"..");
            let index_status = parse_xy_status(xy.chars().next().unwrap_or('.'));
            let working_status = parse_xy_status(xy.chars().nth(1).unwrap_or('.'));
            let path_field = tokens.get(10).unwrap_or(&"");
            let path_part = path_field.split('\t').next().unwrap_or(path_field);

            if matches!(index_status, GitFileStatus::Unmerged)
                || matches!(working_status, GitFileStatus::Unmerged)
            {
                conflict_files.push(unquote_path(path_part));
            }

            files.push(GitStatusFile {
                path: unquote_path(path_part),
                index_status,
                working_status,
            });
        } else if let Some(path) = line.strip_prefix("? ") {
            let path = unquote_path(path);
            files.push(GitStatusFile {
                path: path.clone(),
                index_status: GitFileStatus::Untracked,
                working_status: GitFileStatus::Untracked,
            });
        }
    }

    if !conflict_files.is_empty() {
        in_progress = Some(GitInProgress {
            operation: GitOperation::Merge,
            conflict_files,
        });
    }

    GitStatus {
        branch,
        upstream,
        ahead,
        behind,
        files,
        in_progress,
    }
}

fn parse_xy_status(c: char) -> GitFileStatus {
    match c {
        '.' => GitFileStatus::Unmodified,
        'M' => GitFileStatus::Modified,
        'A' => GitFileStatus::Added,
        'D' => GitFileStatus::Deleted,
        'R' => GitFileStatus::Renamed,
        'C' => GitFileStatus::Copied,
        'U' => GitFileStatus::Unmerged,
        '?' => GitFileStatus::Untracked,
        '!' => GitFileStatus::Ignored,
        _ => GitFileStatus::Unmodified,
    }
}

pub fn parse_diff_output(diff_text: &str, stat_text: &str) -> GitDiffSummary {
    let mut hunks = Vec::new();
    let mut current_hunk_lines: Vec<GitDiffLine> = Vec::new();
    let mut old_path = String::new();
    let mut new_path = String::new();
    let mut hunk_old_path = String::new();
    let mut hunk_new_path = String::new();
    let mut old_start = 0u32;
    let mut old_lines = 0u32;
    let mut new_start = 0u32;
    let mut new_lines = 0u32;
    let mut header = String::new();
    let mut in_hunk = false;
    let mut old_line_counter = 0u32;
    let mut new_line_counter = 0u32;

    for line in diff_text.lines() {
        if let Some(rest) = line.strip_prefix("--- ") {
            old_path = rest.trim_start_matches("a/").to_string();
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            new_path = rest.trim_start_matches("b/").to_string();
        } else if line.starts_with("@@") {
            if in_hunk && !current_hunk_lines.is_empty() {
                hunks.push(GitDiffHunk {
                    old_path: hunk_old_path.clone(),
                    new_path: hunk_new_path.clone(),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    header: header.clone(),
                    lines: std::mem::take(&mut current_hunk_lines),
                });
            }
            header = line.to_string();
            // Snapshot paths at hunk creation: ---/+++ appear only before the
            // first hunk of a file, later hunks must not inherit the next
            // file's headers.
            hunk_old_path = old_path.clone();
            hunk_new_path = new_path.clone();
            if let Some((o, n)) = parse_hunk_header(line) {
                old_start = o.0;
                old_lines = o.1;
                new_start = n.0;
                new_lines = n.1;
                old_line_counter = o.0;
                new_line_counter = n.0;
            }
            in_hunk = true;
        } else if in_hunk {
            if let Some(rest) = line.strip_prefix('+') {
                current_hunk_lines.push(GitDiffLine {
                    kind: GitDiffLineKind::Addition,
                    content: rest.to_string(),
                    old_line: None,
                    new_line: Some(new_line_counter),
                });
                new_line_counter += 1;
            } else if let Some(rest) = line.strip_prefix('-') {
                current_hunk_lines.push(GitDiffLine {
                    kind: GitDiffLineKind::Deletion,
                    content: rest.to_string(),
                    old_line: Some(old_line_counter),
                    new_line: None,
                });
                old_line_counter += 1;
            } else if line.starts_with("\\ No newline") {
                current_hunk_lines.push(GitDiffLine {
                    kind: GitDiffLineKind::NoNewline,
                    content: line.to_string(),
                    old_line: None,
                    new_line: None,
                });
            } else if let Some(rest) = line.strip_prefix(' ') {
                current_hunk_lines.push(GitDiffLine {
                    kind: GitDiffLineKind::Context,
                    content: rest.to_string(),
                    old_line: Some(old_line_counter),
                    new_line: Some(new_line_counter),
                });
                old_line_counter += 1;
                new_line_counter += 1;
            }
        }
    }
    if in_hunk && !current_hunk_lines.is_empty() {
        hunks.push(GitDiffHunk {
            old_path: hunk_old_path,
            new_path: hunk_new_path,
            old_start,
            old_lines,
            new_start,
            new_lines,
            header,
            lines: current_hunk_lines,
        });
    }

    let stat = parse_diff_stat(stat_text);
    GitDiffSummary { hunks, stat }
}

fn parse_hunk_header(line: &str) -> Option<((u32, u32), (u32, u32))> {
    let start = line.find("@@ ")?;
    let end = line[3..].find(" @@")?;
    let core = &line[start + 3..start + 3 + end];
    let parts: Vec<&str> = core.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let old_part = parts[0].strip_prefix('-')?;
    let new_part = parts[1].strip_prefix('+')?;
    let old_nums: Vec<u32> = old_part.split(',').filter_map(|s| s.parse().ok()).collect();
    let new_nums: Vec<u32> = new_part.split(',').filter_map(|s| s.parse().ok()).collect();
    let old_start = *old_nums.first()?;
    let old_lines = old_nums.get(1).copied().unwrap_or(1);
    let new_start = *new_nums.first()?;
    let new_lines = new_nums.get(1).copied().unwrap_or(1);
    Some(((old_start, old_lines), (new_start, new_lines)))
}

fn parse_diff_stat(stat_text: &str) -> GitDiffStat {
    let mut files_changed = 0u32;
    let mut insertions = 0u32;
    let mut deletions = 0u32;
    let mut last_data_line = "";

    for line in stat_text.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("diff ") {
            continue;
        }
        if trimmed.contains("file")
            || (trimmed.contains("insertion") || trimmed.contains("deletion"))
        {
            last_data_line = trimmed;
            break;
        }
        files_changed += 1;
    }

    if last_data_line.is_empty() {
        return GitDiffStat {
            files_changed,
            insertions,
            deletions,
        };
    }

    let mut last_num = 0u32;
    for token in last_data_line.split_whitespace() {
        if let Ok(n) = token.parse::<u32>() {
            last_num = n;
        } else if token.contains("file") {
            files_changed = last_num;
        } else if token.contains("insertion") {
            insertions = last_num;
        } else if token.contains("deletion") {
            deletions = last_num;
        }
    }

    if files_changed == 0 && !hunks_text_is_empty(stat_text) {
        files_changed = 1;
    }

    GitDiffStat {
        files_changed,
        insertions,
        deletions,
    }
}

fn hunks_text_is_empty(s: &str) -> bool {
    s.lines().all(|l| l.trim().is_empty())
}

/// Parse `git show --stat` / `git stash show --stat` trailing summary line.
pub fn parse_commit_stat(stat: &str) -> (u32, u32, u32) {
    let mut files_changed = 0u32;
    let mut insertions = 0u32;
    let mut deletions = 0u32;

    for line in stat.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains("file") || trimmed.contains("insertion") || trimmed.contains("deletion")
        {
            let mut last_num = 0u32;
            for token in trimmed.split_whitespace() {
                if let Ok(n) = token.parse::<u32>() {
                    last_num = n;
                } else if token.contains("file") {
                    files_changed = last_num;
                } else if token.contains("insertion") {
                    insertions = last_num;
                } else if token.contains("deletion") {
                    deletions = last_num;
                }
            }
            break;
        }
        files_changed += 1;
    }
    (insertions, deletions, files_changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_branch_header() {
        let out = "# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n1 .M N... 100644 100644 100644 abc def file.txt\n";
        let s = parse_porcelain_status_v2(out);
        assert_eq!(s.branch, "main");
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 1);
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].path, "file.txt");
        assert!(matches!(s.files[0].index_status, GitFileStatus::Unmodified));
        assert!(matches!(s.files[0].working_status, GitFileStatus::Modified));
    }

    #[test]
    fn porcelain_v2_untracked_and_conflict() {
        let out = "# branch.head main\nu AA 100644 100644 100644 abc def both.txt\n? new.txt\n";
        let s = parse_porcelain_status_v2(out);
        assert_eq!(s.files.len(), 2);
        assert!(matches!(s.files[0].index_status, GitFileStatus::Unmerged));
        let ip = s.in_progress.expect("conflict detected");
        assert_eq!(ip.conflict_files, vec!["both.txt".to_string()]);
    }

    #[test]
    fn diff_output_hunks() {
        let diff = "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,3 @@\n line1\n+line2\n line3\n";
        let stat = " a.txt | 1 +\n 1 file changed, 1 insertion(+)\n";
        let s = parse_diff_output(diff, stat);
        assert_eq!(s.hunks.len(), 1);
        let h = &s.hunks[0];
        assert_eq!(h.old_path, "a.txt");
        assert_eq!(h.new_path, "a.txt");
        assert_eq!(h.old_start, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.lines.len(), 3);
        assert_eq!(s.stat.files_changed, 1);
        assert_eq!(s.stat.insertions, 1);
        assert_eq!(s.stat.deletions, 0);
    }

    #[test]
    fn commit_stat_summary_line() {
        let stat =
            " src/a.rs | 10 +++++++---\n 2 files changed, 12 insertions(+), 5 deletions(-)\n";
        let (ins, del, files) = parse_commit_stat(stat);
        assert_eq!((ins, del, files), (12, 5, 2));
    }

    #[test]
    fn malformed_inputs_are_tolerated() {
        // truncated porcelain lines
        let s = parse_porcelain_status_v2("1 .M N... 100644\n");
        assert!(s.files.is_empty());
        let s = parse_porcelain_status_v2("2 R. N... 100644\n");
        assert!(s.files.is_empty());
        // non-numeric branch.ab counters
        let s = parse_porcelain_status_v2("# branch.ab +x -y\n");
        assert_eq!((s.ahead, s.behind), (0, 0));
        // unparseable hunk header is tolerated (0-start hunk)
        let d = parse_diff_output("@@ -nope +x @@\n line\n", "");
        assert_eq!(d.hunks.len(), 1);
        assert_eq!(d.hunks[0].old_start, 0);
        // stat without a summary line counts the file line
        let s = parse_diff_output("", " nothing here\n");
        assert_eq!(s.stat.files_changed, 1);
        // empty commit stat
        assert_eq!(parse_commit_stat(""), (0, 0, 0));
        // rename line (2 ) with two-token path field must not panic
        let out =
            "# branch.head main\n2 R. N... 100644 100644 100644 abc def R100 old.txt\tnew.txt\n";
        let s = parse_porcelain_status_v2(out);
        assert!(s.files.len() <= 1);
    }
}
