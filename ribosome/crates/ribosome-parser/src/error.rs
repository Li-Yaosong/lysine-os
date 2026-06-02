use std::fmt;

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A single semantic validation problem with field path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
    pub severity: Severity,
}

impl ValidationIssue {
    pub fn error(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            severity: Severity::Error,
        }
    }

    pub fn warning(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            severity: Severity::Warning,
        }
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// Parser-level errors.
#[derive(Debug, thiserror::Error)]
pub enum ParserError {
    #[error("YAML syntax error: {0}")]
    YamlSyntax(#[from] serde_yaml::Error),

    #[error("mRNA validation failed with {} error(s)", .issues.iter().filter(|i| i.severity == Severity::Error).count())]
    Validation { issues: Vec<ValidationIssue> },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl ParserError {
    pub fn validation(issues: Vec<ValidationIssue>) -> Self {
        Self::Validation { issues }
    }

    pub fn issues(&self) -> Option<&[ValidationIssue]> {
        match self {
            Self::Validation { issues } => Some(issues),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, ParserError>;
