use ribosome_parser::parse_mrna;

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
