#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("repository not found: {0}")]
    NotFound(String),

    #[error("repository sync failed: {0}")]
    SyncFailed(String),

    #[error("package index error: {0}")]
    IndexError(String),
}

pub type Result<T> = std::result::Result<T, RepositoryError>;
