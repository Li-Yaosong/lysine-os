use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::error::{RepositoryError, Result};
use crate::index::{IndexDepends, IndexEntry, RepositoryIndex};

/// Represents an opened or created nucleus repository on disk.
pub struct Repository {
    pub root: PathBuf,
}

/// Categories within a repository.
pub const CATEGORIES: &[&str] = &["core", "devel", "desktop", "ai", "extra"];

impl Repository {
    /// Create a new empty repository at the given root path.
    pub fn create(root: &Path) -> Result<Self> {
        if root.exists() && root.join("nucleus.db").exists() {
            return Err(RepositoryError::IndexError(format!(
                "repository already exists at {}",
                root.display()
            )));
        }

        std::fs::create_dir_all(root).map_err(|e| RepositoryError::Io {
            path: root.to_path_buf(),
            reason: e.to_string(),
        })?;

        // Create category subdirectories.
        for cat in CATEGORIES {
            let cat_dir = root.join(cat);
            std::fs::create_dir_all(&cat_dir).map_err(|e| RepositoryError::Io {
                path: cat_dir,
                reason: e.to_string(),
            })?;
        }

        // Create empty index.
        let index = RepositoryIndex::default();
        let index_path = RepositoryIndex::default_index_path(root);
        index.save(&index_path)?;

        info!(path = %root.display(), "created empty repository");
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Open an existing repository.
    pub fn open(root: &Path) -> Result<Self> {
        let index_path = RepositoryIndex::default_index_path(root);
        if !index_path.exists() {
            return Err(RepositoryError::NotFound(format!(
                "no nucleus.db found at {}",
                root.display()
            )));
        }
        info!(path = %root.display(), "opened repository");
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Load the current index from disk.
    pub fn load_index(&self) -> Result<RepositoryIndex> {
        let index_path = RepositoryIndex::default_index_path(&self.root);
        RepositoryIndex::load(&index_path)
    }

    /// Save an index back to disk.
    pub fn save_index(&self, index: &RepositoryIndex) -> Result<()> {
        let index_path = RepositoryIndex::default_index_path(&self.root);
        index.save(&index_path)
    }

    /// Publish a `.prot` file into the repository.
    ///
    /// The file is copied into the appropriate category directory and the index is updated.
    /// `category` must be one of the valid categories (core, devel, desktop, ai, extra).
    pub fn publish(&self, prot_path: &Path, category: &str) -> Result<()> {
        if !CATEGORIES.contains(&category) {
            return Err(RepositoryError::InvalidPackage {
                path: prot_path.to_path_buf(),
                reason: format!(
                    "invalid category '{category}', must be one of: {}",
                    CATEGORIES.join(", ")
                ),
            });
        }

        if !prot_path.exists() {
            return Err(RepositoryError::NotFound(format!(
                "package file not found: {}",
                prot_path.display()
            )));
        }

        let filename = prot_path
            .file_name()
            .ok_or_else(|| RepositoryError::InvalidPackage {
                path: prot_path.to_path_buf(),
                reason: "no file name".to_string(),
            })?
            .to_string_lossy()
            .to_string();

        // Validate filename format: <name>-<version>-<release>-<arch>.prot
        if !filename.ends_with(".prot") {
            return Err(RepositoryError::InvalidPackage {
                path: prot_path.to_path_buf(),
                reason: format!("file must have .prot extension, got: {filename}"),
            });
        }

        // Parse metadata from filename using the shared parser.
        let stem = &filename[..filename.len() - 5]; // strip .prot
        let parsed = parse_prot_filename(stem);

        // Copy file into category directory.
        let dest_dir = self.root.join(category);
        let dest_path = dest_dir.join(&filename);
        std::fs::copy(prot_path, &dest_path).map_err(|e| RepositoryError::Io {
            path: dest_path.clone(),
            reason: e.to_string(),
        })?;

        // Compute hash.
        let sha256 = hash_file(&dest_path)?;

        // Compute installed size (compressed size as approximation).
        let installed_size = std::fs::metadata(&dest_path)
            .map_err(|e| RepositoryError::Io {
                path: dest_path.clone(),
                reason: e.to_string(),
            })?
            .len();

        // Build index entry using parsed filename components + META metadata.
        let prot_meta = match ribosome_package::read_meta(&dest_path) {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    path = %dest_path.display(),
                    error = %e,
                    "failed to read META from .prot, using empty defaults"
                );
                ribosome_package::ProtMeta::default()
            }
        };

        let (description, license) = extract_mrna_fields(&prot_meta.mrna_yaml);

        let entry = IndexEntry {
            name: parsed.name,
            version: parsed.version,
            release: parsed.release,
            description,
            license,
            arch: parsed.arch,
            category: category.to_string(),
            filename: format!("{category}/{filename}"),
            sha256,
            depends: IndexDepends {
                runtime: prot_meta.depends_runtime,
                build: prot_meta.depends_build,
            },
            provides: vec![],
            conflicts: vec![],
            installed_size,
            build_date: chrono::Utc::now().to_rfc3339(),
        };

        // Update index.
        let mut index = self.load_index()?;
        let pkg_name = entry.name.clone();
        let pkg_version = entry.version.clone();
        index.add_entry(entry);
        self.save_index(&index)?;

        info!(package = %pkg_name, version = %pkg_version, category, "published package");
        Ok(())
    }

    /// Rebuild the index by scanning all `.prot` files in the repository.
    pub fn rebuild_index(&self) -> Result<usize> {
        let mut index = RepositoryIndex::default();
        let mut count = 0usize;

        for cat in CATEGORIES {
            let cat_dir = self.root.join(cat);
            if !cat_dir.exists() {
                continue;
            }
            let entries = std::fs::read_dir(&cat_dir).map_err(|e| RepositoryError::Io {
                path: cat_dir.clone(),
                reason: e.to_string(),
            })?;

            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("prot") {
                    continue;
                }

                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let sha256 = hash_file(&path)?;
                let installed_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

                let stem = &filename[..filename.len() - 5];
                let parsed = parse_prot_filename(stem);

                // Read META for dependency info and description.
                let prot_meta = match ribosome_package::read_meta(&path) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "failed to read META during rebuild, using empty defaults"
                        );
                        ribosome_package::ProtMeta::default()
                    }
                };

                let (description, license) = extract_mrna_fields(&prot_meta.mrna_yaml);

                let index_entry = IndexEntry {
                    name: parsed.name,
                    version: parsed.version,
                    release: parsed.release,
                    description,
                    license,
                    arch: parsed.arch,
                    category: cat.to_string(),
                    filename: format!("{cat}/{filename}"),
                    sha256,
                    depends: IndexDepends {
                        runtime: prot_meta.depends_runtime,
                        build: prot_meta.depends_build,
                    },
                    provides: vec![],
                    conflicts: vec![],
                    installed_size,
                    build_date: String::new(),
                };

                index.add_entry(index_entry);
                count += 1;
            }
        }

        self.save_index(&index)?;
        info!(total = count, "rebuilt repository index");
        Ok(count)
    }
}

struct ParsedFilename {
    name: String,
    version: String,
    release: u32,
    arch: String,
}

/// Parse a .prot stem like "bash-5.2.37-1-x86_64" into components.
fn parse_prot_filename(stem: &str) -> ParsedFilename {
    // Format: <name>-<version>-<release>-<arch>
    // arch is always the last segment
    let parts: Vec<&str> = stem.rsplitn(2, '-').collect();
    let (name_ver_release, arch) = if parts.len() == 2 {
        (parts[1], parts[0].to_string())
    } else {
        return ParsedFilename {
            name: stem.to_string(),
            version: String::new(),
            release: 1,
            arch: String::new(),
        };
    };

    // name_ver_release: "bash-5.2.37-1"
    let parts2: Vec<&str> = name_ver_release.rsplitn(2, '-').collect();
    let (name_ver, release_str) = if parts2.len() == 2 {
        (parts2[1], parts2[0])
    } else {
        return ParsedFilename {
            name: name_ver_release.to_string(),
            version: String::new(),
            release: 1,
            arch,
        };
    };

    let release = release_str.parse::<u32>().unwrap_or(1);

    // name_ver: "bash-5.2.37"
    // The name is everything up to the last '-' that is followed by a version-like string.
    let name = extract_name(name_ver);
    let version = extract_version(name_ver);

    ParsedFilename {
        name,
        version,
        release,
        arch,
    }
}

/// Extract the package name from "name-version" string.
fn extract_name(name_ver: &str) -> String {
    // Find the last '-' followed by a digit (start of version).
    let mut last_split = 0;
    for (i, c) in name_ver.char_indices() {
        if c == '-'
            && name_ver
                .get(i + 1..)
                .is_some_and(|s| s.starts_with(char::is_numeric))
        {
            last_split = i;
        }
    }
    if last_split > 0 {
        name_ver[..last_split].to_string()
    } else {
        name_ver.to_string()
    }
}

/// Extract the version from "name-version" string.
fn extract_version(name_ver: &str) -> String {
    let mut last_split = 0;
    for (i, c) in name_ver.char_indices() {
        if c == '-'
            && name_ver
                .get(i + 1..)
                .is_some_and(|s| s.starts_with(char::is_numeric))
        {
            last_split = i;
        }
    }
    if last_split > 0 {
        name_ver[last_split + 1..].to_string()
    } else {
        String::new()
    }
}

/// Extract `description` and `license` fields from raw mRNA YAML content.
/// Returns empty strings if parsing fails or fields are missing.
fn extract_mrna_fields(mrna_yaml: &Option<String>) -> (String, String) {
    let Some(yaml) = mrna_yaml else {
        return (String::new(), String::new());
    };

    match ribosome_parser::parse_mrna(yaml) {
        Ok(mrna) => (mrna.description, mrna.license),
        Err(_) => (String::new(), String::new()),
    }
}

fn hash_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path).map_err(|e| RepositoryError::Io {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    std::io::copy(&mut file, &mut hasher).map_err(|e| RepositoryError::Io {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let result = hasher.finalize();
    Ok(format!("sha256:{result:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_filename() {
        let parsed = parse_prot_filename("bash-5.2.37-1-x86_64");
        assert_eq!(parsed.name, "bash");
        assert_eq!(parsed.version, "5.2.37");
        assert_eq!(parsed.release, 1);
        assert_eq!(parsed.arch, "x86_64");
    }

    #[test]
    fn parse_multi_segment_name() {
        let parsed = parse_prot_filename("linux-api-headers-6.18.0-1-x86_64");
        assert_eq!(parsed.name, "linux-api-headers");
        assert_eq!(parsed.version, "6.18.0");
        assert_eq!(parsed.release, 1);
        assert_eq!(parsed.arch, "x86_64");
    }

    #[test]
    fn extract_name_version_separation() {
        assert_eq!(extract_name("bash-5.2.37"), "bash");
        assert_eq!(extract_version("bash-5.2.37"), "5.2.37");
        assert_eq!(
            extract_name("linux-api-headers-6.18.0"),
            "linux-api-headers"
        );
        assert_eq!(extract_version("linux-api-headers-6.18.0"), "6.18.0");
    }

    #[test]
    fn repository_create_and_open() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");

        let _repo = Repository::create(&repo_path).unwrap();
        assert!(repo_path.join("nucleus.db").exists());
        assert!(repo_path.join("core").exists());

        // Opening should work.
        let repo2 = Repository::open(&repo_path).unwrap();
        assert_eq!(repo2.root, repo_path);
    }

    #[test]
    fn repository_double_create_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");
        Repository::create(&repo_path).unwrap();
        assert!(Repository::create(&repo_path).is_err());
    }

    #[test]
    fn index_add_find_remove() {
        let mut index = RepositoryIndex::default();
        let entry = IndexEntry {
            name: "bash".to_string(),
            version: "5.2.37".to_string(),
            release: 1,
            description: "Shell".to_string(),
            license: "GPL-3.0".to_string(),
            arch: "x86_64".to_string(),
            category: "core".to_string(),
            filename: "core/bash-5.2.37-1-x86_64.prot".to_string(),
            sha256: "sha256:abc".to_string(),
            depends: Default::default(),
            provides: vec![],
            conflicts: vec![],
            installed_size: 1024,
            build_date: String::new(),
        };

        index.add_entry(entry.clone());
        assert_eq!(index.len(), 1);

        let found = index.find("bash").unwrap();
        assert_eq!(found.version, "5.2.37");

        let removed = index.remove_entry("bash").unwrap();
        assert_eq!(removed.name, "bash");
        assert!(index.is_empty());
    }

    #[test]
    fn index_search() {
        let mut index = RepositoryIndex::default();
        index.add_entry(IndexEntry {
            name: "bash".to_string(),
            version: "5.2".to_string(),
            release: 1,
            description: "GNU Bourne Again Shell".to_string(),
            license: "GPL".to_string(),
            arch: "x86_64".to_string(),
            category: "core".to_string(),
            filename: "core/bash.prot".to_string(),
            sha256: String::new(),
            depends: Default::default(),
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            build_date: String::new(),
        });
        index.add_entry(IndexEntry {
            name: "zsh".to_string(),
            version: "5.9".to_string(),
            release: 1,
            description: "Z shell".to_string(),
            license: "MIT".to_string(),
            arch: "x86_64".to_string(),
            category: "core".to_string(),
            filename: "core/zsh.prot".to_string(),
            sha256: String::new(),
            depends: Default::default(),
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            build_date: String::new(),
        });

        let results = index.search("shell");
        assert_eq!(results.len(), 2);

        let results = index.search("bourne");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "bash");

        let results = index.search("nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn index_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let index_path = tmp.path().join("nucleus.db");

        let mut index = RepositoryIndex::default();
        index.add_entry(IndexEntry {
            name: "bash".to_string(),
            version: "5.2.37".to_string(),
            release: 1,
            description: "Shell".to_string(),
            license: "GPL".to_string(),
            arch: "x86_64".to_string(),
            category: "core".to_string(),
            filename: "core/bash-5.2.37-1-x86_64.prot".to_string(),
            sha256: "sha256:abc".to_string(),
            depends: Default::default(),
            provides: vec![],
            conflicts: vec![],
            installed_size: 1024,
            build_date: "2026-06-04T00:00:00Z".to_string(),
        });

        index.save(&index_path).unwrap();
        assert!(index_path.exists());
        assert!(tmp.path().join("nucleus.db.sha256").exists());

        let loaded = RepositoryIndex::load(&index_path).unwrap();
        assert_eq!(loaded.len(), 1);
        let entry = loaded.find("bash").unwrap();
        assert_eq!(entry.version, "5.2.37");
    }
}
