use ribosome_parser::{parse_mrna, parse_mrna_file, ParserError};

const FEATURES_YAML: &str = r#"
api-version: 1
name: gcc
version: 14.2.0
release: 1
description: GNU Compiler Collection
license: GPL-3.0-or-later
features:
  default: [lto]
  options:
    lto:
      description: Link-time optimization
      cflags: -flto=auto
    cxx:
      description: C++ language support
sources:
  - url: https://ftp.gnu.org/gnu/gcc/gcc-14.2.0/gcc-14.2.0.tar.xz
    hash: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
build:
  install: make install
outputs:
  main:
    description: main package
  lib:
    description: libraries
    files:
      - /usr/lib/*.so*
"#;

#[test]
fn parse_features_and_outputs() {
    let mrna = parse_mrna(FEATURES_YAML).expect("valid gcc subset");
    assert!(mrna.features.is_some());
    assert!(mrna.outputs.is_some());
}

#[test]
fn parse_conditional_patch() {
    let yaml = r#"
api-version: 1
name: bash
version: 5.2.37
release: 1
description: Bourne Again Shell
license: GPL-3.0-or-later
patches:
  - fix.patch
  - aarch64.patch:
      condition: 'arch == "aarch64"'
sources:
  - url: https://ftp.gnu.org/gnu/bash/bash-5.2.37.tar.gz
    hash: sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
build:
  install: make install
"#;
    let mrna = parse_mrna(yaml).expect("bash with patches");
    assert_eq!(mrna.patches.as_ref().map(|p| p.len()), Some(2));
}

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn valid_fixtures_all_parse() {
    let valid_dir = fixtures_dir().join("valid");
    let mut tested = 0;
    for entry in std::fs::read_dir(&valid_dir).expect("valid fixtures dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let mrna = parse_mrna_file(&path)
            .unwrap_or_else(|e| panic!("{} should be valid: {e}", path.display()));
        assert!(!mrna.name.is_empty(), "{} has empty name", path.display());
        tested += 1;
    }
    assert!(
        tested >= 3,
        "expected at least 3 valid fixtures, got {tested}"
    );
}

#[test]
fn invalid_fixtures_all_fail() {
    let invalid_dir = fixtures_dir().join("invalid");
    let mut tested = 0;
    for entry in std::fs::read_dir(&invalid_dir).expect("invalid fixtures dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let result = parse_mrna_file(&path);
        assert!(
            result.is_err(),
            "{} should be invalid but parsed successfully",
            path.display()
        );
        if let Err(ParserError::Validation { issues }) = &result {
            assert!(
                issues
                    .iter()
                    .any(|i| matches!(i.severity, ribosome_parser::Severity::Error)),
                "{} should have at least one Error-level issue",
                path.display()
            );
        }
        tested += 1;
    }
    assert!(
        tested >= 3,
        "expected at least 3 invalid fixtures, got {tested}"
    );
}

#[test]
fn invalid_multi_error_has_multiple_errors() {
    let path = fixtures_dir().join("invalid").join("multi-error.yaml");
    let err = parse_mrna_file(&path).expect_err("multi-error.yaml should fail");
    let ParserError::Validation { issues } = &err else {
        panic!("expected validation error");
    };
    let error_count = issues
        .iter()
        .filter(|i| matches!(i.severity, ribosome_parser::Severity::Error))
        .count();
    assert!(
        error_count >= 5,
        "expected at least 5 errors, got {error_count}"
    );
}
