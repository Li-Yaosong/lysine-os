#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("sandbox creation failed: {0}")]
    CreationFailed(String),

    #[error("sandbox execution failed: {0}")]
    ExecutionFailed(String),

    #[error("namespace setup failed: {0}")]
    // Reserved for direct Linux namespace API (future iteration)
    NamespaceError(String),
}

pub type Result<T> = std::result::Result<T, SandboxError>;
