#[derive(Debug, thiserror::Error)]
pub enum DepsError {
    #[error("circular dependency detected: {cycle}")]
    CircularDependency { cycle: String },

    #[error("missing dependency: {0}")]
    MissingDependency(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("failed to parse mRNA {path}: {reason}")]
    Parse { path: String, reason: String },
}

pub type Result<T> = std::result::Result<T, DepsError>;
