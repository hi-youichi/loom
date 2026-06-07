use std::fmt;

#[derive(Debug)]
pub enum ConvertError {
    ProfileNotFound(String),
    UnknownFormat(String),
    Io(std::io::Error),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertError::ProfileNotFound(name) => write!(f, "agent profile not found: {name}"),
            ConvertError::UnknownFormat(fmt) => write!(f, "unknown export format: {fmt}"),
            ConvertError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for ConvertError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConvertError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ConvertError {
    fn from(e: std::io::Error) -> Self {
        ConvertError::Io(e)
    }
}

impl From<crate::profile::ProfileError> for ConvertError {
    fn from(e: crate::profile::ProfileError) -> Self {
        ConvertError::ProfileNotFound(e.to_string())
    }
}
