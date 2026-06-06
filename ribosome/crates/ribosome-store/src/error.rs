use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("I/O error at {path}: {message}")]
    Io { path: PathBuf, message: String },

    #[error("invalid digest format: {0}")]
    InvalidDigest(String),

    #[error("object not found: {0}")]
    NotFound(String),

    #[error("corrupt object: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("ref already exists: {namespace}/{name}")]
    RefExists { namespace: String, name: String },

    #[error("GC failed: {0}")]
    GcFailed(String),
}

impl StoreError {
    pub fn io(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Io {
            path: path.into(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        // Callers should use StoreError::io() with path context.
        // This impl is a fallback for cases where no path is available.
        Self::Io {
            path: PathBuf::new(),
            message: e.to_string(),
        }
    }
}
