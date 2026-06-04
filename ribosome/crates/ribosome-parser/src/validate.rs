use std::sync::LazyLock;

use regex::Regex;
use spdx::Expression;

use crate::dependency::parse_dependency_spec;
use crate::error::ValidationIssue;
use crate::model::{MrnaFile, PatchItem};

const SUPPORTED_API_VERSION: u32 = 1;

static KEBAB_CASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$").expect("valid kebab-case regex"));
static SHA256_HASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^sha256:[0-9a-fA-F]{64}$").expect("valid sha256 regex"));

/// Collect all validation issues (errors and warnings).
pub fn collect_validation_issues(mrna: &MrnaFile) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    validate_api_version(mrna, &mut issues);
    validate_name(mrna, &mut issues);
    validate_version(mrna, &mut issues);
    validate_release(mrna, &mut issues);
    validate_description(mrna, &mut issues);
    validate_license(mrna, &mut issues);
    validate_sources(mrna, &mut issues);
    validate_build(mrna, &mut issues);
    validate_depends(mrna, &mut issues);
    validate_features(mrna, &mut issues);
    validate_outputs(mrna, &mut issues);
    validate_patches(mrna, &mut issues);

    issues
}

fn validate_api_version(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    if mrna.api_version != SUPPORTED_API_VERSION {
        issues.push(ValidationIssue::error(
            "api-version",
            format!(
                "unsupported api-version {} (expected {})",
                mrna.api_version, SUPPORTED_API_VERSION
            ),
        ));
    }
}

fn validate_name(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    if mrna.name.trim().is_empty() {
        issues.push(ValidationIssue::error("name", "must not be empty"));
        return;
    }
    if !KEBAB_CASE.is_match(&mrna.name) {
        issues.push(ValidationIssue::error(
            "name",
            "must be kebab-case (lowercase letters, digits, hyphens)",
        ));
    }
}

fn validate_version(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    if mrna.version.trim().is_empty() {
        issues.push(ValidationIssue::error("version", "must not be empty"));
    }
}

fn validate_release(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    if mrna.release < 1 {
        issues.push(ValidationIssue::error("release", "must be >= 1"));
    }
}

fn validate_description(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    if mrna.description.trim().is_empty() {
        issues.push(ValidationIssue::error("description", "must not be empty"));
    }
}

fn validate_license(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    if mrna.license.trim().is_empty() {
        issues.push(ValidationIssue::error("license", "must not be empty"));
        return;
    }
    if Expression::parse(&mrna.license).is_err() {
        issues.push(ValidationIssue::warning(
            "license",
            format!("unable to verify SPDX identifier: {}", mrna.license),
        ));
    }
}

fn validate_sources(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    if mrna.sources.is_empty() {
        issues.push(ValidationIssue::error(
            "sources",
            "must contain at least one source",
        ));
        return;
    }

    for (i, source) in mrna.sources.iter().enumerate() {
        let prefix = format!("sources[{i}]");

        if let Err(e) = url::Url::parse(&source.url) {
            issues.push(ValidationIssue::error(
                format!("{prefix}.url"),
                format!("invalid URL: {e}"),
            ));
        } else if let Ok(parsed) = url::Url::parse(&source.url) {
            let scheme = parsed.scheme();
            if !matches!(scheme, "http" | "https" | "ftp") {
                issues.push(ValidationIssue::error(
                    format!("{prefix}.url"),
                    format!("unsupported URL scheme: {scheme} (expected http, https, or ftp)"),
                ));
            }
        }

        if let Some(hash) = &source.hash {
            if !SHA256_HASH.is_match(hash) {
                issues.push(ValidationIssue::error(
                    format!("{prefix}.hash"),
                    "invalid sha256 format (expected sha256: + 64 hex digits)",
                ));
            }
        }

        if let Some(sig) = &source.signature {
            if sig != "gpg" && sig != "minisign" {
                issues.push(ValidationIssue::error(
                    format!("{prefix}.signature"),
                    format!("unsupported signature type: {sig} (expected gpg or minisign)"),
                ));
            }
        }
    }
}

fn validate_build(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    match &mrna.build {
        None => {
            issues.push(ValidationIssue::error(
                "build",
                "build block is required (must include at least an install step)",
            ));
        }
        Some(build) => match &build.install {
            None => issues.push(ValidationIssue::error(
                "build.install",
                "install step is required when build block is present",
            )),
            Some(s) if s.trim().is_empty() => issues.push(ValidationIssue::error(
                "build.install",
                "install step must not be empty",
            )),
            _ => {}
        },
    }
}

fn validate_depends(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    let Some(depends) = &mrna.depends else {
        return;
    };

    for (section, dep_str) in depends.all_dependency_strings() {
        let field = section.to_string();
        match parse_dependency_spec(dep_str) {
            Ok(spec) => {
                if spec.name == mrna.name {
                    issues.push(ValidationIssue::error(
                        field,
                        format!("package must not depend on itself: {}", mrna.name),
                    ));
                }
            }
            Err(e) => issues.push(ValidationIssue::error(field, e)),
        }
    }
}

fn validate_features(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    let Some(features) = &mrna.features else {
        return;
    };

    for feat in &features.default {
        if !features.options.contains_key(feat) {
            issues.push(ValidationIssue::error(
                "features.default".to_string(),
                format!("default feature '{feat}' is not defined in features.options"),
            ));
        }
    }

    for (name, opt) in &features.options {
        if opt.description.trim().is_empty() {
            issues.push(ValidationIssue::error(
                format!("features.options.{name}.description"),
                "description must not be empty",
            ));
        }
        if let Some(deps) = &opt.depends {
            for (i, dep_str) in deps.iter().enumerate() {
                let field = format!("features.options.{name}.depends[{i}]");
                if let Err(e) = parse_dependency_spec(dep_str) {
                    issues.push(ValidationIssue::error(field, e));
                }
            }
        }
    }
}

fn validate_outputs(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    let Some(outputs) = &mrna.outputs else {
        return;
    };

    if !outputs.entries.contains_key("main") {
        issues.push(ValidationIssue::warning(
            "outputs",
            "outputs should include a 'main' entry",
        ));
    }

    for (name, entry) in &outputs.entries {
        if entry.description.trim().is_empty() {
            issues.push(ValidationIssue::error(
                format!("outputs.{name}.description"),
                "description must not be empty",
            ));
        }
        if let Some(files) = &entry.files {
            for (i, pattern) in files.iter().enumerate() {
                if let Some(msg) = validate_glob_pattern(pattern) {
                    issues.push(ValidationIssue::warning(
                        format!("outputs.{name}.files[{i}]"),
                        msg,
                    ));
                }
            }
        }
    }
}

fn validate_patches(mrna: &MrnaFile, issues: &mut Vec<ValidationIssue>) {
    let Some(patches) = &mrna.patches else {
        return;
    };

    for (i, patch) in patches.iter().enumerate() {
        match patch {
            PatchItem::Simple(name) => {
                if name.trim().is_empty() {
                    issues.push(ValidationIssue::error(
                        format!("patches[{i}]"),
                        "patch file name must not be empty",
                    ));
                }
            }
            PatchItem::Conditional {
                name, condition, ..
            } => {
                if name.trim().is_empty() {
                    issues.push(ValidationIssue::error(
                        format!("patches[{i}]"),
                        "patch file name must not be empty",
                    ));
                }
                if condition.as_ref().is_none_or(|c| c.trim().is_empty()) {
                    issues.push(ValidationIssue::warning(
                        format!("patches[{i}].condition"),
                        "conditional patch should include a non-empty condition",
                    ));
                }
            }
        }
    }
}

fn validate_glob_pattern(pattern: &str) -> Option<String> {
    if pattern.trim().is_empty() {
        return Some("glob pattern must not be empty".to_string());
    }
    let stars = pattern.matches('*').count();
    if pattern.contains("**") && stars > pattern.matches("**").count() * 2 {
        // allow ** only
    }
    if pattern.ends_with('\\') {
        return Some("glob pattern ends with escape".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::error::Severity;
    use crate::model::*;
    use crate::validate::collect_validation_issues;

    /// Helper: build a minimal valid MrnaFile for mutating in tests.
    fn base_mrna() -> MrnaFile {
        MrnaFile {
            api_version: 1,
            name: "test-pkg".to_string(),
            version: "1.0.0".to_string(),
            release: 1,
            description: "A test package".to_string(),
            homepage: None,
            license: "MIT".to_string(),
            maintainer: None,
            tags: None,
            depends: None,
            features: None,
            sources: vec![Source {
                url: "https://example.com/test-1.0.0.tar.xz".to_string(),
                hash: Some(
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
                signature: None,
                key_id: None,
            }],
            patches: None,
            build: Some(Build {
                prepare: None,
                compile: None,
                check: None,
                install: Some("make install".to_string()),
            }),
            post_install: None,
            post_remove: None,
            outputs: None,
        }
    }

    fn has_error(mrna: &MrnaFile, field: &str) -> bool {
        collect_validation_issues(mrna)
            .iter()
            .any(|i| i.severity == Severity::Error && i.field == field)
    }

    fn has_warning(mrna: &MrnaFile, field: &str) -> bool {
        collect_validation_issues(mrna)
            .iter()
            .any(|i| i.severity == Severity::Warning && i.field == field)
    }

    // V01: api-version must equal 1
    #[test]
    fn v01_api_version_valid() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "api-version"));
    }

    #[test]
    fn v01_api_version_invalid() {
        let mut mrna = base_mrna();
        mrna.api_version = 99;
        assert!(has_error(&mrna, "api-version"));
    }

    // V02: name must be kebab-case
    #[test]
    fn v02_name_valid() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "name"));
    }

    #[test]
    fn v02_name_empty() {
        let mut mrna = base_mrna();
        mrna.name = "  ".to_string();
        assert!(has_error(&mrna, "name"));
    }

    #[test]
    fn v02_name_uppercase_rejected() {
        let mut mrna = base_mrna();
        mrna.name = "BadName".to_string();
        assert!(has_error(&mrna, "name"));
    }

    #[test]
    fn v02_name_underscore_rejected() {
        let mut mrna = base_mrna();
        mrna.name = "bad_name".to_string();
        assert!(has_error(&mrna, "name"));
    }

    #[test]
    fn v02_name_digit_start_rejected() {
        let mut mrna = base_mrna();
        mrna.name = "9base".to_string();
        assert!(has_error(&mrna, "name"));
    }

    // V03: version must not be empty
    #[test]
    fn v03_version_valid() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "version"));
    }

    #[test]
    fn v03_version_empty() {
        let mut mrna = base_mrna();
        mrna.version = "  ".to_string();
        assert!(has_error(&mrna, "version"));
    }

    // V04: release >= 1
    #[test]
    fn v04_release_valid() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "release"));
    }

    #[test]
    fn v04_release_zero() {
        let mut mrna = base_mrna();
        mrna.release = 0;
        assert!(has_error(&mrna, "release"));
    }

    // V05: description must not be empty
    #[test]
    fn v05_description_valid() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "description"));
    }

    #[test]
    fn v05_description_empty() {
        let mut mrna = base_mrna();
        mrna.description = "  ".to_string();
        assert!(has_error(&mrna, "description"));
    }

    // V06: license SPDX warning
    #[test]
    fn v06_license_valid_spdx() {
        let mrna = base_mrna();
        assert!(!has_warning(&mrna, "license"));
    }

    #[test]
    fn v06_license_empty() {
        let mut mrna = base_mrna();
        mrna.license = "  ".to_string();
        assert!(has_error(&mrna, "license"));
    }

    #[test]
    fn v06_license_invalid_spdx_warns() {
        let mut mrna = base_mrna();
        mrna.license = "NotALicense".to_string();
        assert!(has_warning(&mrna, "license"));
    }

    // V07: sources.len >= 1
    #[test]
    fn v07_sources_valid() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "sources"));
    }

    #[test]
    fn v07_sources_empty() {
        let mut mrna = base_mrna();
        mrna.sources = vec![];
        assert!(has_error(&mrna, "sources"));
    }

    // V08: source URL scheme must be http/https/ftp
    #[test]
    fn v08_url_https_valid() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "sources[0].url"));
    }

    #[test]
    fn v08_url_ftp_valid() {
        let mut mrna = base_mrna();
        mrna.sources[0].url = "ftp://ftp.example.com/test.tar.gz".to_string();
        assert!(!has_error(&mrna, "sources[0].url"));
    }

    #[test]
    fn v08_url_file_scheme_rejected() {
        let mut mrna = base_mrna();
        mrna.sources[0].url = "file:///tmp/test.tar.gz".to_string();
        assert!(has_error(&mrna, "sources[0].url"));
    }

    #[test]
    fn v08_url_invalid_rejected() {
        let mut mrna = base_mrna();
        mrna.sources[0].url = "not-a-url".to_string();
        assert!(has_error(&mrna, "sources[0].url"));
    }

    // V09: source hash format
    #[test]
    fn v09_hash_valid_sha256() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "sources[0].hash"));
    }

    #[test]
    fn v09_hash_none_ok() {
        let mut mrna = base_mrna();
        mrna.sources[0].hash = None;
        assert!(!has_error(&mrna, "sources[0].hash"));
    }

    #[test]
    fn v09_hash_short_rejected() {
        let mut mrna = base_mrna();
        mrna.sources[0].hash = Some("sha256:abc".to_string());
        assert!(has_error(&mrna, "sources[0].hash"));
    }

    #[test]
    fn v09_hash_no_prefix_rejected() {
        let mut mrna = base_mrna();
        mrna.sources[0].hash =
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
        assert!(has_error(&mrna, "sources[0].hash"));
    }

    // V10: signature type
    #[test]
    fn v10_signature_gpg_valid() {
        let mut mrna = base_mrna();
        mrna.sources[0].signature = Some("gpg".to_string());
        assert!(!has_error(&mrna, "sources[0].signature"));
    }

    #[test]
    fn v10_signature_minisign_valid() {
        let mut mrna = base_mrna();
        mrna.sources[0].signature = Some("minisign".to_string());
        assert!(!has_error(&mrna, "sources[0].signature"));
    }

    #[test]
    fn v10_signature_invalid_rejected() {
        let mut mrna = base_mrna();
        mrna.sources[0].signature = Some("pgp".to_string());
        assert!(has_error(&mrna, "sources[0].signature"));
    }

    // V11: build.install required when build block present
    #[test]
    fn v11_build_install_present_ok() {
        let mrna = base_mrna();
        assert!(!has_error(&mrna, "build.install"));
    }

    #[test]
    fn v11_build_install_missing() {
        let mut mrna = base_mrna();
        mrna.build = Some(Build {
            prepare: None,
            compile: None,
            check: None,
            install: None,
        });
        assert!(has_error(&mrna, "build.install"));
    }

    #[test]
    fn v11_build_install_whitespace() {
        let mut mrna = base_mrna();
        mrna.build = Some(Build {
            prepare: None,
            compile: None,
            check: None,
            install: Some("  ".to_string()),
        });
        assert!(has_error(&mrna, "build.install"));
    }

    #[test]
    fn v11_no_build_block_rejected() {
        let mut mrna = base_mrna();
        mrna.build = None;
        assert!(has_error(&mrna, "build"));
    }

    // V12: depends format must be parseable
    #[test]
    fn v12_depends_valid() {
        let mut mrna = base_mrna();
        mrna.depends = Some(Depends {
            build: Some(vec!["glibc >= 2.39".to_string()]),
            runtime: None,
            check: None,
        });
        assert!(!has_error(&mrna, "depends.build"));
    }

    #[test]
    fn v12_depends_invalid_format() {
        let mut mrna = base_mrna();
        mrna.depends = Some(Depends {
            build: Some(vec!["bad name >= 1".to_string()]),
            runtime: None,
            check: None,
        });
        assert!(has_error(&mrna, "depends.build"));
    }

    // V13: self-dependency rejected
    #[test]
    fn v13_self_dependency_rejected() {
        let mut mrna = base_mrna();
        mrna.depends = Some(Depends {
            build: Some(vec!["test-pkg".to_string()]),
            runtime: None,
            check: None,
        });
        assert!(has_error(&mrna, "depends.build"));
    }

    // V14: features.default must reference defined options
    #[test]
    fn v14_features_default_valid() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let mut options = HashMap::new();
        options.insert(
            "lto".to_string(),
            FeatureOption {
                description: "Link-time optimization".to_string(),
                depends: None,
                cflags: None,
            },
        );
        mrna.features = Some(Features {
            default: vec!["lto".to_string()],
            options,
        });
        assert!(!has_error(&mrna, "features.default"));
    }

    #[test]
    fn v14_features_default_undefined() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let options = HashMap::new();
        mrna.features = Some(Features {
            default: vec!["nonexistent".to_string()],
            options,
        });
        assert!(has_error(&mrna, "features.default"));
    }

    // V15: features.options.*.depends format
    #[test]
    fn v15_feature_depends_valid() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let mut options = HashMap::new();
        options.insert(
            "cxx".to_string(),
            FeatureOption {
                description: "C++ support".to_string(),
                depends: Some(vec!["glibc >= 2.39".to_string()]),
                cflags: None,
            },
        );
        mrna.features = Some(Features {
            default: vec![],
            options,
        });
        assert!(!has_error(&mrna, "features.options.cxx.depends[0]"));
    }

    #[test]
    fn v15_feature_depends_invalid() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let mut options = HashMap::new();
        options.insert(
            "cxx".to_string(),
            FeatureOption {
                description: "C++ support".to_string(),
                depends: Some(vec!["bad name".to_string()]),
                cflags: None,
            },
        );
        mrna.features = Some(Features {
            default: vec![],
            options,
        });
        assert!(has_error(&mrna, "features.options.cxx.depends[0]"));
    }

    // V16: outputs should include 'main' (warning)
    #[test]
    fn v16_outputs_with_main_ok() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let mut entries = HashMap::new();
        entries.insert(
            "main".to_string(),
            OutputEntry {
                description: "main package".to_string(),
                files: None,
            },
        );
        mrna.outputs = Some(Outputs { entries });
        assert!(!has_warning(&mrna, "outputs"));
    }

    #[test]
    fn v16_outputs_without_main_warns() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let mut entries = HashMap::new();
        entries.insert(
            "lib".to_string(),
            OutputEntry {
                description: "libraries".to_string(),
                files: None,
            },
        );
        mrna.outputs = Some(Outputs { entries });
        assert!(has_warning(&mrna, "outputs"));
    }

    // V17: outputs files glob basic check
    #[test]
    fn v17_glob_valid() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let mut entries = HashMap::new();
        entries.insert(
            "main".to_string(),
            OutputEntry {
                description: "main".to_string(),
                files: Some(vec!["/usr/lib/*.so".to_string()]),
            },
        );
        mrna.outputs = Some(Outputs { entries });
        assert!(!has_warning(&mrna, "outputs.main.files[0]"));
    }

    #[test]
    fn v17_glob_empty_warns() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let mut entries = HashMap::new();
        entries.insert(
            "main".to_string(),
            OutputEntry {
                description: "main".to_string(),
                files: Some(vec!["  ".to_string()]),
            },
        );
        mrna.outputs = Some(Outputs { entries });
        assert!(has_warning(&mrna, "outputs.main.files[0]"));
    }

    #[test]
    fn v17_glob_trailing_escape_warns() {
        use std::collections::HashMap;
        let mut mrna = base_mrna();
        let mut entries = HashMap::new();
        entries.insert(
            "main".to_string(),
            OutputEntry {
                description: "main".to_string(),
                files: Some(vec!["/usr/lib/foo\\".to_string()]),
            },
        );
        mrna.outputs = Some(Outputs { entries });
        assert!(has_warning(&mrna, "outputs.main.files[0]"));
    }

    // V18: patches condition should not be empty
    #[test]
    fn v18_patch_simple_valid() {
        let mut mrna = base_mrna();
        mrna.patches = Some(vec![PatchItem::Simple("fix.patch".to_string())]);
        assert!(!has_error(&mrna, "patches[0]"));
    }

    #[test]
    fn v18_patch_simple_empty_rejected() {
        let mut mrna = base_mrna();
        mrna.patches = Some(vec![PatchItem::Simple("  ".to_string())]);
        assert!(has_error(&mrna, "patches[0]"));
    }

    #[test]
    fn v18_conditional_patch_with_condition_ok() {
        let mut mrna = base_mrna();
        mrna.patches = Some(vec![PatchItem::Conditional {
            name: "aarch64.patch".to_string(),
            condition: Some("arch == aarch64".to_string()),
            severity: None,
        }]);
        assert!(!has_warning(&mrna, "patches[0].condition"));
    }

    #[test]
    fn v18_conditional_patch_no_condition_warns() {
        let mut mrna = base_mrna();
        mrna.patches = Some(vec![PatchItem::Conditional {
            name: "aarch64.patch".to_string(),
            condition: None,
            severity: None,
        }]);
        assert!(has_warning(&mrna, "patches[0].condition"));
    }

    #[test]
    fn v18_conditional_patch_empty_condition_warns() {
        let mut mrna = base_mrna();
        mrna.patches = Some(vec![PatchItem::Conditional {
            name: "aarch64.patch".to_string(),
            condition: Some("  ".to_string()),
            severity: None,
        }]);
        assert!(has_warning(&mrna, "patches[0].condition"));
    }
}
