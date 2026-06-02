/// Core error types for the Ribosome build engine.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("build failed for package '{package}': {reason}")]
    BuildFailed { package: String, reason: String },

    #[error("package not found: {0}")]
    PackageNotFound(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
