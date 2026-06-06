pub mod deps;
pub mod info;
pub mod install;
pub mod list;
pub mod remove;
pub mod search;
pub mod update;

use std::path::Path;

use anyhow::{Context, Result};
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
pub fn hash_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    std::io::copy(&mut file, &mut hasher)?;
    let result = hasher.finalize();
    Ok(format!("sha256:{result:x}"))
}
