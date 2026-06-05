use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("repository not found: {0}")]
    NotFound(String),

    #[error("repository sync failed: {0}")]
    SyncFailed(String),

    #[error("package index error: {0}")]
    IndexError(String),

    #[error("package not found in index: {0}")]
    PackageNotFound(String),

    #[error("I/O error at {path}: {reason}")]
    Io { path: PathBuf, reason: String },

    #[error("invalid package file: {path}: {reason}")]
    InvalidPackage { path: PathBuf, reason: String },

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
}

pub type Result<T> = std::result::Result<T, RepositoryError>;
