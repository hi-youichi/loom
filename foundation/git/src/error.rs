/// Classified git failure kinds. Map to extension error codes at the boundary:
/// NotFound -> -32003, InvalidParams -> -32001, Conflict -> -32005,
/// Forbidden -> -32002, everything else -> -32603 internal_error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitErrorKind {
    NotFound,
    InvalidParams,
    Conflict,
    Forbidden,
    Locked,
    Auth,
    Unsupported,
    GitMissing,
    Io,
    Internal,
}

#[derive(Debug, thiserror::Error)]
pub struct GitError {
    kind: GitErrorKind,
    message: String,
    stdout: Option<String>,
    stderr: Option<String>,
}

impl GitError {
    pub fn new(kind: GitErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            stdout: None,
            stderr: None,
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(GitErrorKind::NotFound, message)
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(GitErrorKind::InvalidParams, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(GitErrorKind::Conflict, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(GitErrorKind::Forbidden, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(GitErrorKind::Internal, message)
    }

    pub fn unsupported(operation: &str) -> Self {
        Self::new(
            GitErrorKind::Unsupported,
            format!("backend does not implement: {operation}"),
        )
    }

    pub fn from_io(operation: &str, e: std::io::Error) -> Self {
        let kind = if e.kind() == std::io::ErrorKind::NotFound {
            GitErrorKind::GitMissing
        } else {
            GitErrorKind::Io
        };
        Self::new(kind, format!("{operation}: {e}"))
    }

    pub fn kind(&self) -> GitErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn stdout(&self) -> Option<&str> {
        self.stdout.as_deref()
    }

    pub fn stderr(&self) -> Option<&str> {
        self.stderr.as_deref()
    }

    pub fn is_unsupported(&self) -> bool {
        self.kind == GitErrorKind::Unsupported
    }

    pub fn is_locked(&self) -> bool {
        self.kind == GitErrorKind::Locked
    }

    /// Data payload for the extension layer: stderr when present, else message.
    pub fn data(&self) -> String {
        self.stderr.clone().unwrap_or_else(|| self.message.clone())
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)?;
        if let Some(stderr) = &self.stderr {
            write!(f, "\nstderr: {stderr}")?;
        }
        Ok(())
    }
}

impl From<std::io::Error> for GitError {
    fn from(e: std::io::Error) -> Self {
        GitError::from_io("git io error", e)
    }
}

/// Classify stderr from a failed git invocation, preserving the exact
/// classification the extension layer relied on pre-migration.
pub(crate) fn classify_stderr(stderr: &str) -> Option<GitError> {
    if stderr.contains("not a git repository") || stderr.contains("fatal: not a git") {
        return Some(GitError::not_found("not a git repository"));
    }
    if stderr.contains("does not exist")
        || stderr.contains("unknown revision")
        || stderr.contains("does not have any commits yet")
        || stderr.contains("did not match any files")
    {
        return Some(GitError::not_found(stderr));
    }
    if stderr.contains("index.lock") || stderr.contains("Unable to create") {
        return Some(GitError {
            kind: GitErrorKind::Locked,
            message: "index lock conflict".to_string(),
            stdout: None,
            stderr: Some(stderr.to_string()),
        });
    }
    None
}

pub(crate) fn internal_with_output(message: String, stdout: String, stderr: String) -> GitError {
    GitError {
        kind: GitErrorKind::Internal,
        message,
        stdout: Some(stdout),
        stderr: Some(stderr),
    }
}

pub type GitResult<T> = Result<T, GitError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_not_a_repo() {
        let e = classify_stderr("fatal: not a git repository (or any of the parent directories)")
            .unwrap();
        assert_eq!(e.kind(), GitErrorKind::NotFound);
        assert_eq!(e.message(), "not a git repository");
        assert!(e.data().contains("not a git repository"));
    }

    #[test]
    fn classify_missing_paths() {
        for msg in [
            "path 'nope.txt' does not exist in 'HEAD'",
            "fatal: ambiguous argument 'x': unknown revision",
        ] {
            let e = classify_stderr(msg).unwrap();
            assert_eq!(e.kind(), GitErrorKind::NotFound);
            assert_eq!(
                e.data(),
                msg,
                "missing-path errors carry the raw stderr as data"
            );
        }
    }

    #[test]
    fn classify_locked() {
        let e = classify_stderr("error: unable to create 'C:/x/.git/index.lock': File exists.")
            .unwrap();
        assert_eq!(e.kind(), GitErrorKind::Locked);
        assert!(e.is_locked());
        assert!(e.data().contains("index.lock"));
    }

    #[test]
    fn classify_unknown_passes_through() {
        assert!(classify_stderr("random failure").is_none());
    }

    #[test]
    fn constructors_and_accessors() {
        let e = GitError::conflict("boom");
        assert_eq!(e.kind(), GitErrorKind::Conflict);
        assert!(!e.is_unsupported());
        assert_eq!(e.stderr(), None);
        assert_eq!(e.data(), "boom");

        let uns = GitError::unsupported("status");
        assert!(uns.is_unsupported());
        assert_eq!(uns.kind(), GitErrorKind::Unsupported);

        let inner = internal_with_output("cmd failed".into(), "out".into(), "err".into());
        assert_eq!(inner.stdout(), Some("out"));
        assert_eq!(inner.data(), "err");

        let fmt = format!("{inner}");
        assert!(fmt.contains("cmd failed"));
        assert!(fmt.contains("stderr: err"));
    }

    #[test]
    fn io_error_maps_git_missing() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "git not found");
        let e = GitError::from_io("spawn git", io);
        assert_eq!(e.kind(), GitErrorKind::GitMissing);
    }
}
