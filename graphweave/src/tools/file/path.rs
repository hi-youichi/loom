//! Path validation for file tools: resolve path under working folder and prevent escape.
//!
//! Used by all file tools to ensure paths stay under `working_folder` (canonical).
//! Interacts with [`ToolSourceError`](crate::tool_source::ToolSourceError) for invalid paths.

use std::path::{Component, Path, PathBuf};

use crate::tool_source::ToolSourceError;

/// Normalizes a path by resolving `.` and `..` without requiring the path to exist.
///
/// Used to validate that a joined path (working_folder + param) does not escape
/// the working folder. Does not resolve symlinks.
fn normalize_path(path: &Path) -> PathBuf {
    let mut buf = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => buf = PathBuf::from(p.as_os_str()),
            Component::RootDir => buf.push(comp),
            Component::CurDir => {}
            Component::ParentDir => {
                buf.pop();
            }
            Component::Normal(s) => buf.push(s),
        }
    }
    buf
}

/// Resolves a path parameter under the canonical working folder.
///
/// Joins `path_param` (relative to `working_folder`) and ensures the result
/// is under `working_folder`. If the resolved path exists, returns its
/// canonical form (resolving symlinks). Otherwise returns the normalized path.
///
/// # Errors
///
/// - `InvalidInput` if `working_folder` is not a valid directory or the
///   resolved path is outside the working folder.
/// - `Transport` if canonicalization of an existing path fails.
///
/// # Interaction
///
/// Called by each file tool before performing filesystem operations.
pub fn resolve_path_under(
    working_folder: &Path,
    path_param: &str,
) -> Result<PathBuf, ToolSourceError> {
    let base_canonical = working_folder.canonicalize().map_err(|e| {
        ToolSourceError::InvalidInput(format!(
            "working folder not found or not a directory: {}",
            e
        ))
    })?;

    let path_param = path_param.trim();
    let path_param = if path_param.is_empty() {
        "."
    } else {
        path_param
    };
    let full = base_canonical.join(path_param);
    let normalized = normalize_path(&full);

    if !normalized.starts_with(&base_canonical) {
        return Err(ToolSourceError::InvalidInput(
            "path is outside working folder".to_string(),
        ));
    }

    if normalized.exists() {
        normalized
            .canonicalize()
            .map_err(|e| ToolSourceError::Transport(format!("failed to resolve path: {}", e)))
    } else {
        Ok(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_resolves_dot_dot() {
        let p = Path::new("/a/b/../c");
        let n = normalize_path(p);
        assert_eq!(n, PathBuf::from("/a/c"));
    }

    #[test]
    fn normalize_path_resolves_dot() {
        let p = Path::new("/a/./b");
        let n = normalize_path(p);
        assert_eq!(n, PathBuf::from("/a/b"));
    }
}
