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
    if let Some(build) = &mrna.build {
        match &build.install {
            None => issues.push(ValidationIssue::error(
                "build.install",
                "install step is required when build block is present",
            )),
            Some(s) if s.trim().is_empty() => issues.push(ValidationIssue::error(
                "build.install",
                "install step must not be empty",
            )),
            _ => {}
        }
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
    use crate::dependency::is_valid_package_name;

    #[test]
    fn kebab_case_valid() {
        assert!(is_valid_package_name("linux-api-headers"));
        assert!(!is_valid_package_name("Bad_Name"));
    }
}
