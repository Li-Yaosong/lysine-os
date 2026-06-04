use std::path::PathBuf;

/// Core error types for the Ribosome build engine.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("build failed for package '{package}': {reason}")]
    BuildFailed { package: String, reason: String },

    #[error("package not found: {0}")]
    PackageNotFound(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("source download failed for '{package}': {url}")]
    SourceDownloadFailed { package: String, url: String },

    #[error("hash mismatch for '{package}': expected {expected}, computed {computed}")]
    HashMismatch {
        package: String,
        expected: String,
        computed: String,
    },

    #[error("I/O error at {path}: {reason}")]
    Io { path: PathBuf, reason: String },

    #[error("shell command failed in phase '{phase}' of {package}: {message}")]
    CommandFailed {
        package: String,
        phase: String,
        message: String,
    },
}

impl CoreError {
    pub fn io(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self::Io {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
