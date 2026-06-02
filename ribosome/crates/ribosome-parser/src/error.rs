#[derive(Debug, thiserror::Error)]
pub enum ParserError {
    #[error("YAML parse error: {0}")]
    YamlError(#[from] serde_yaml::Error),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("invalid mRNA format: {0}")]
    InvalidFormat(String),
}

pub type Result<T> = std::result::Result<T, ParserError>;
