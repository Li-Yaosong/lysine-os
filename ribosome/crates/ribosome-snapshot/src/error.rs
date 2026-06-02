#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot creation failed: {0}")]
    CreationFailed(String),

    #[error("snapshot not found: {0}")]
    NotFound(String),

    #[error("rollback failed: {0}")]
    RollbackFailed(String),

    #[error("Btrfs error: {0}")]
    BtrfsError(String),
}

pub type Result<T> = std::result::Result<T, SnapshotError>;
