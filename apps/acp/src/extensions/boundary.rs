//! Directory and worktree boundary validation.
//!
//! Ensures all filesystem operations stay within the session's working directory.

use std::path::{Path, PathBuf};

use super::ExtensionError;

pub fn validate_path(
    path: &str,
    working_directory: Option<&Path>,
) -> Result<PathBuf, ExtensionError> {
    let resolved = match working_directory {
        Some(base) => base.join(path),
        None => PathBuf::from(path),
    };

    let canonical = resolved
        .canonicalize()
        .map_err(|_| ExtensionError::not_found(format!("path does not exist: {path}")))?;

    if let Some(base) = working_directory {
        let base_canonical = base
            .canonicalize()
            .map_err(|_| ExtensionError::directory_boundary_violation(&base.to_string_lossy()))?;
        if !canonical.starts_with(&base_canonical) {
            return Err(ExtensionError::directory_boundary_violation(path));
        }
    }

    Ok(canonical)
}

pub fn is_within_boundary(path: &Path, base: &Path) -> bool {
    let canonical_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let canonical_base = match base.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    canonical_path.starts_with(&canonical_base)
}
