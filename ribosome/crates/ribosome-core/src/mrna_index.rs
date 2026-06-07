//! Multi-version mRNA index.
//!
//! Scans a nucleus directory for .mRNA files, collects all available versions
//! per package, and resolves which version to use based on a `PackageSpec`.
//!
//! Supports a version lock file (TOML format) to pin exact versions for
//! reproducible builds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ribosome_parser::parse_mrna_file;
use tracing::{info, warn};

use crate::error::{CoreError, Result};
use crate::profile::PackageSpec;

/// A single mRNA file entry in the index.
#[derive(Debug, Clone)]
pub struct MrnaEntry {
    /// Package version string.
    pub version: String,
    /// Filesystem path to the .mRNA file.
    pub path: PathBuf,
}

/// Multi-version index of mRNA files, keyed by package name.
#[derive(Debug, Clone, Default)]
pub struct MrnaIndex {
    /// package_name → sorted list of entries
    packages: HashMap<String, Vec<MrnaEntry>>,
    /// Version lock: package_name → pinned version (from versions.lock)
    locked: HashMap<String, String>,
}

impl MrnaIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan a directory recursively for .mRNA files and build the index.
    ///
    /// All versions of each package are preserved and sorted lexicographically
    /// (ascending), so the last element is always the "latest".
    pub fn scan(dir: &Path) -> Result<Self> {
        let mut index = Self::new();

        if !dir.exists() {
            return Ok(index);
        }

        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("mRNA") {
                continue;
            }
            if let Ok(mrna) = parse_mrna_file(path) {
                index
                    .packages
                    .entry(mrna.name.clone())
                    .or_default()
                    .push(MrnaEntry {
                        version: mrna.version.clone(),
                        path: path.to_path_buf(),
                    });
            }
        }

        // Sort each version list ascending so that last() is the latest
        for versions in index.packages.values_mut() {
            versions.sort_by(|a, b| a.version.cmp(&b.version));
        }

        Ok(index)
    }

    /// Load version lock from a TOML file.
    ///
    /// The file format is simple `key = "value"` lines:
    /// ```toml
    /// linux-kernel = "7.0"
    /// gcc = "14.2.0"
    /// ```
    ///
    /// Lines starting with `#` are comments, blank lines are ignored.
    /// Returns the number of entries loaded.
    pub fn load_version_lock(&mut self, path: &Path) -> Result<usize> {
        if !path.exists() {
            return Ok(0);
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| CoreError::io(path, format!("failed to read version lock: {e}")))?;

        let mut count = 0;
        for line in content.lines() {
            let line = line.trim();
            // Skip comments and blank lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Parse: name = "version"
            if let Some((name, value)) = line.split_once('=') {
                let name = name.trim();
                let value = value.trim().trim_matches('"');
                if !name.is_empty() && !value.is_empty() {
                    self.locked.insert(name.to_string(), value.to_string());
                    count += 1;
                }
            }
        }

        info!(path = %path.display(), entries = count, "loaded version lock");
        Ok(count)
    }

    /// Convenience: scan a directory and optionally load a version lock file.
    pub fn scan_with_lock(nucleus_dir: &Path, lock_file: Option<&Path>) -> Result<Self> {
        let mut index = Self::scan(nucleus_dir)?;
        if let Some(lock) = lock_file {
            index.load_version_lock(lock)?;
        }
        Ok(index)
    }

    /// Number of distinct packages in the index.
    pub fn package_count(&self) -> usize {
        self.packages.len()
    }

    /// Number of version-locked packages.
    pub fn locked_count(&self) -> usize {
        self.locked.len()
    }

    /// Get all available versions for a package.
    pub fn versions(&self, name: &str) -> Option<&[MrnaEntry]> {
        self.packages.get(name).map(|v| v.as_slice())
    }

    /// Get the locked version for a package, if any.
    pub fn locked_version(&self, name: &str) -> Option<&str> {
        self.locked.get(name).map(|s| s.as_str())
    }

    /// Resolve which mRNA file to use for a given `PackageSpec`.
    ///
    /// Resolution priority (highest to lowest):
    /// 1. `spec.version` explicit pin → exact match
    /// 2. Version lock file entry → exact match
    /// 3. Fallback → latest available version
    ///
    /// Returns `None` if the package name is not in the index at all.
    pub fn resolve(&self, spec: &PackageSpec) -> Option<&MrnaEntry> {
        let versions = self.packages.get(&spec.name)?;

        // Priority 1: explicit pin from PackageSpec
        if let Some(ref wanted) = spec.version {
            return self.find_version(&spec.name, wanted, versions);
        }

        // Priority 2: version lock file
        if let Some(locked_ver) = self.locked.get(&spec.name) {
            return self.find_version(&spec.name, locked_ver, versions);
        }

        // Priority 3: latest
        versions.last()
    }

    /// Find an exact version in the list, with warning on mismatch.
    fn find_version<'a>(
        &self,
        name: &str,
        wanted: &str,
        versions: &'a [MrnaEntry],
    ) -> Option<&'a MrnaEntry> {
        for entry in versions {
            if entry.version == wanted {
                return Some(entry);
            }
        }
        warn!(
            package = name,
            wanted = wanted,
            available = ?versions.iter().map(|e| &e.version).collect::<Vec<_>>(),
            "requested version not found, falling back to latest"
        );
        versions.last()
    }

    /// Collect all unique package names in the index.
    pub fn package_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.packages.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_nonexistent_dir_returns_empty() {
        let idx = MrnaIndex::scan(Path::new("/nonexistent")).unwrap();
        assert_eq!(idx.package_count(), 0);
    }

    #[test]
    fn resolve_returns_latest_when_no_version_pin() {
        let mut idx = MrnaIndex::new();
        idx.packages.insert(
            "linux-kernel".to_string(),
            vec![
                MrnaEntry {
                    version: "6.18.0".to_string(),
                    path: PathBuf::from("/a/6.18.0.mRNA"),
                },
                MrnaEntry {
                    version: "7.0".to_string(),
                    path: PathBuf::from("/a/7.0.mRNA"),
                },
            ],
        );

        let spec = PackageSpec::name_only("linux-kernel");
        let entry = idx.resolve(&spec).unwrap();
        assert_eq!(entry.version, "7.0");
        assert_eq!(entry.path, PathBuf::from("/a/7.0.mRNA"));
    }

    #[test]
    fn resolve_returns_pinned_version() {
        let mut idx = MrnaIndex::new();
        idx.packages.insert(
            "linux-kernel".to_string(),
            vec![
                MrnaEntry {
                    version: "6.18.0".to_string(),
                    path: PathBuf::from("/a/6.18.0.mRNA"),
                },
                MrnaEntry {
                    version: "7.0".to_string(),
                    path: PathBuf::from("/a/7.0.mRNA"),
                },
            ],
        );

        let spec = PackageSpec::pinned("linux-kernel", "6.18.0");
        let entry = idx.resolve(&spec).unwrap();
        assert_eq!(entry.version, "6.18.0");
    }

    #[test]
    fn resolve_returns_none_for_unknown_package() {
        let idx = MrnaIndex::new();
        let spec = PackageSpec::name_only("no-such-pkg");
        assert!(idx.resolve(&spec).is_none());
    }

    #[test]
    fn resolve_single_version_always_picked() {
        let mut idx = MrnaIndex::new();
        idx.packages.insert(
            "bash".to_string(),
            vec![MrnaEntry {
                version: "5.2".to_string(),
                path: PathBuf::from("/b/5.2.mRNA"),
            }],
        );

        let spec = PackageSpec::name_only("bash");
        let entry = idx.resolve(&spec).unwrap();
        assert_eq!(entry.version, "5.2");
    }

    #[test]
    fn package_names_returns_sorted() {
        let mut idx = MrnaIndex::new();
        idx.packages.insert(
            "gcc".to_string(),
            vec![MrnaEntry {
                version: "14.2.0".to_string(),
                path: PathBuf::from("/g/14.2.0.mRNA"),
            }],
        );
        idx.packages.insert(
            "bash".to_string(),
            vec![MrnaEntry {
                version: "5.2".to_string(),
                path: PathBuf::from("/b/5.2.mRNA"),
            }],
        );

        assert_eq!(idx.package_names(), vec!["bash", "gcc"]);
    }

    #[test]
    fn load_version_lock_parses_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_path = tmp.path().join("versions.lock");
        std::fs::write(
            &lock_path,
            r#"
# comment
linux-kernel = "7.0"
gcc = "14.2.0"

bash = "5.2.37"
"#,
        )
        .unwrap();

        let mut idx = MrnaIndex::new();
        let count = idx.load_version_lock(&lock_path).unwrap();
        assert_eq!(count, 3);
        assert_eq!(idx.locked_version("linux-kernel"), Some("7.0"));
        assert_eq!(idx.locked_version("gcc"), Some("14.2.0"));
        assert_eq!(idx.locked_version("bash"), Some("5.2.37"));
        assert_eq!(idx.locked_version("unknown"), None);
    }

    #[test]
    fn resolve_uses_lock_when_no_spec_pin() {
        let mut idx = MrnaIndex::new();
        idx.packages.insert(
            "linux-kernel".to_string(),
            vec![
                MrnaEntry {
                    version: "6.18.0".to_string(),
                    path: PathBuf::from("/a/6.18.0.mRNA"),
                },
                MrnaEntry {
                    version: "7.0".to_string(),
                    path: PathBuf::from("/a/7.0.mRNA"),
                },
            ],
        );
        idx.locked
            .insert("linux-kernel".to_string(), "6.18.0".to_string());

        // name_only spec should use lock, not latest
        let spec = PackageSpec::name_only("linux-kernel");
        let entry = idx.resolve(&spec).unwrap();
        assert_eq!(entry.version, "6.18.0");
    }

    #[test]
    fn spec_pin_overrides_lock() {
        let mut idx = MrnaIndex::new();
        idx.packages.insert(
            "linux-kernel".to_string(),
            vec![
                MrnaEntry {
                    version: "6.18.0".to_string(),
                    path: PathBuf::from("/a/6.18.0.mRNA"),
                },
                MrnaEntry {
                    version: "7.0".to_string(),
                    path: PathBuf::from("/a/7.0.mRNA"),
                },
            ],
        );
        // Lock says 6.18.0, but spec pins 7.0 — spec wins
        idx.locked
            .insert("linux-kernel".to_string(), "6.18.0".to_string());

        let spec = PackageSpec::pinned("linux-kernel", "7.0");
        let entry = idx.resolve(&spec).unwrap();
        assert_eq!(entry.version, "7.0");
    }

    #[test]
    fn load_nonexistent_lock_returns_zero() {
        let mut idx = MrnaIndex::new();
        let count = idx
            .load_version_lock(Path::new("/nonexistent/versions.lock"))
            .unwrap();
        assert_eq!(count, 0);
    }
}
