#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("package creation failed: {0}")]
    CreationFailed(String),

    #[error("package extraction failed: {0}")]
    ExtractionFailed(String),

    #[error("hash verification failed: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, PackageError>;
