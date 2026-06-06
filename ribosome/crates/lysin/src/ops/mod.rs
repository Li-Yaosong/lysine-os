pub mod deps;
pub mod info;
pub mod install;
pub mod list;
pub mod remove;
pub mod search;
pub mod update;

use std::path::Path;

use anyhow::Result;
use ribosome_parser::Version;

/// Compare two version strings using semantic versioning.
/// Returns `true` if `newer` is strictly greater than `current`.
pub fn version_is_newer(newer: &str, current: &str) -> bool {
    match (Version::parse(newer), Version::parse(current)) {
        (Ok(n), Ok(c)) => n > c,
        _ => newer > current,
    }
}

/// Compare two version strings using semantic versioning.
/// Returns `true` if `newer` is greater than or equal to `current`.
pub fn version_is_newer_or_equal(newer: &str, current: &str) -> bool {
    match (Version::parse(newer), Version::parse(current)) {
        (Ok(n), Ok(c)) => n >= c,
        _ => newer >= current,
    }
}

/// Compute SHA-256 of a file, returning "sha256:<hex>".
///
/// Delegates to `ribosome_store::hash_file` for project-wide consistency.
pub fn hash_file(path: &Path) -> Result<String> {
    ribosome_store::hash_file(path)
        .map_err(|e| anyhow::anyhow!("failed to hash {}: {e}", path.display()))
}
