//! mRNA YAML parser and validator for the Ribosome build system.

pub mod dependency;
pub mod error;
pub mod model;
pub mod validate;
pub mod version;

pub use dependency::{parse_dependency_spec, DependencySpec};
pub use error::{ParserError, Result, Severity, ValidationIssue};
pub use model::{
    Build, Depends, FeatureOption, Features, MrnaFile, OutputEntry, Outputs, PatchItem, Source,
};
pub use validate::collect_validation_issues;
pub use version::{Version, VersionConstraint};

use std::path::Path;

/// Parse and validate mRNA from a YAML string.
pub fn parse_mrna(content: &str) -> Result<MrnaFile> {
    let mrna: MrnaFile = serde_yaml::from_str(content)?;
    apply_validation(mrna)
}

/// Read a file and parse/validate mRNA.
pub fn parse_mrna_file(path: &Path) -> Result<MrnaFile> {
    let content = std::fs::read_to_string(path)?;
    parse_mrna(&content)
}

/// Validate an already-deserialized mRNA document.
pub fn validate_mrna(mrna: &MrnaFile) -> Result<()> {
    let issues = collect_validation_issues(mrna);
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .cloned()
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ParserError::validation(errors))
    }
}

/// Return only warning-level validation issues.
pub fn validate_warnings(mrna: &MrnaFile) -> Vec<ValidationIssue> {
    collect_validation_issues(mrna)
        .into_iter()
        .filter(|i| i.severity == Severity::Warning)
        .collect()
}

fn apply_validation(mrna: MrnaFile) -> Result<MrnaFile> {
    let issues = collect_validation_issues(&mrna);
    let has_errors = issues.iter().any(|i| i.severity == Severity::Error);
    if has_errors {
        Err(ParserError::validation(issues))
    } else {
        Ok(mrna)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
api-version: 1
name: zlib
version: 1.3.1
release: 1
description: Compression library
license: Zlib
sources:
  - url: https://zlib.net/zlib-1.3.1.tar.xz
    hash: sha256:16455bf0addbd0f1241910a512f7e7b72a7aff05932ad9a105eb061e9119bfe1
build:
  install: |
    make DESTDIR="$DESTDIR" install
"#;

    #[test]
    fn parse_minimal_valid() {
        let mrna = parse_mrna(MINIMAL).expect("should parse");
        assert_eq!(mrna.name, "zlib");
    }

    #[test]
    fn reject_bad_api_version() {
        let yaml = MINIMAL.replace("api-version: 1", "api-version: 99");
        let err = parse_mrna(&yaml).unwrap_err();
        assert!(matches!(err, ParserError::Validation { .. }));
    }

    #[test]
    fn reject_invalid_hash() {
        let yaml = MINIMAL.replace(
            "sha256:16455bf0addbd0f1241910a512f7e7b72a7aff05932ad9a105eb061e9119bfe1",
            "sha256:abc",
        );
        assert!(parse_mrna(&yaml).is_err());
    }

    #[test]
    fn reject_self_dependency() {
        let yaml = r#"
api-version: 1
name: zlib
version: 1.3.1
release: 1
description: Compression library
license: Zlib
depends:
  build:
    - zlib
sources:
  - url: https://zlib.net/zlib-1.3.1.tar.xz
build:
  install: make install
"#;
        assert!(parse_mrna(yaml).is_err());
    }
}
