use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::error::{RepositoryError, Result};

const INDEX_FILENAME: &str = "nucleus.db";
const INDEX_HASH_FILENAME: &str = "nucleus.db.sha256";

/// A single entry in the repository index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub version: String,
    pub release: u32,
    pub description: String,
    pub license: String,
    pub arch: String,
    pub category: String,
    pub filename: String,
    pub sha256: String,
    #[serde(default)]
    pub depends: IndexDepends,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    pub installed_size: u64,
    pub build_date: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexDepends {
    #[serde(default)]
    pub runtime: Vec<String>,
    #[serde(default)]
    pub build: Vec<String>,
}

/// The repository index, backed by a JSON Lines file.
#[derive(Debug, Clone, Default)]
pub struct RepositoryIndex {
    /// name -> entry (latest version wins on duplicates).
    entries: HashMap<String, IndexEntry>,
    /// Ordered list for deterministic iteration.
    order: Vec<String>,
}

impl RepositoryIndex {
    /// Load index from a `nucleus.db` JSON Lines file.
    pub fn load(index_path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(index_path).map_err(|e| RepositoryError::Io {
            path: index_path.to_path_buf(),
            reason: e.to_string(),
        })?;

        let mut index = Self::default();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match serde_json::from_str::<IndexEntry>(line) {
                Ok(entry) => index.add_entry(entry),
                Err(e) => {
                    warn!(line = &line[..line.len().min(80)], error = %e, "skipping malformed index line");
                }
            }
        }
        Ok(index)
    }

    /// Save index to a `nucleus.db` JSON Lines file, plus a `.sha256` sidecar.
    pub fn save(&self, index_path: &Path) -> Result<()> {
        let mut content = String::new();
        for name in &self.order {
            if let Some(entry) = self.entries.get(name) {
                let json = serde_json::to_string(entry)
                    .map_err(|e| RepositoryError::IndexError(format!("serialize error: {e}")))?;
                content.push_str(&json);
                content.push('\n');
            }
        }

        // Write atomically via tmp file.
        let tmp_path = index_path.with_extension("db.tmp");
        std::fs::write(&tmp_path, &content).map_err(|e| RepositoryError::Io {
            path: tmp_path.clone(),
            reason: e.to_string(),
        })?;
        std::fs::rename(&tmp_path, index_path).map_err(|e| RepositoryError::Io {
            path: index_path.to_path_buf(),
            reason: e.to_string(),
        })?;

        // Write SHA-256 sidecar.
        let hash_path = index_path.with_extension("db.sha256");
        let hash = sha256_str(&content);
        let hash_line = format!("sha256:{hash}  {}\n", INDEX_FILENAME);
        std::fs::write(&hash_path, &hash_line).map_err(|e| RepositoryError::Io {
            path: hash_path,
            reason: e.to_string(),
        })?;

        info!(entries = self.entries.len(), "saved repository index");
        Ok(())
    }

    /// Add or replace an entry. If an entry with the same name exists,
    /// the one with the higher version (semver) + release wins.
    pub fn add_entry(&mut self, entry: IndexEntry) {
        let should_insert = match self.entries.get(&entry.name) {
            Some(existing) => {
                let new_ver = ribosome_parser::Version::parse(&entry.version);
                let old_ver = ribosome_parser::Version::parse(&existing.version);
                match (new_ver, old_ver) {
                    (Ok(n), Ok(o)) if n > o => true,
                    (Ok(n), Ok(o)) if n == o => entry.release >= existing.release,
                    // Fallback to lexicographic for non-standard versions.
                    _ => entry.version >= existing.version,
                }
            }
            None => true,
        };
        if should_insert {
            if !self.entries.contains_key(&entry.name) {
                self.order.push(entry.name.clone());
            }
            self.entries.insert(entry.name.clone(), entry);
        }
    }

    /// Remove an entry by name.
    pub fn remove_entry(&mut self, name: &str) -> Option<IndexEntry> {
        self.order.retain(|n| n != name);
        self.entries.remove(name)
    }

    /// Find an entry by exact package name.
    pub fn find(&self, name: &str) -> Option<&IndexEntry> {
        self.entries.get(name)
    }

    /// Find an entry by exact package name, mutable.
    pub fn find_mut(&mut self, name: &str) -> Option<&mut IndexEntry> {
        self.entries.get_mut(name)
    }

    /// Search entries whose name or description contains the keyword (case-insensitive).
    pub fn search(&self, keyword: &str) -> Vec<&IndexEntry> {
        let kw = keyword.to_lowercase();
        self.order
            .iter()
            .filter_map(|name| {
                let entry = self.entries.get(name)?;
                if entry.name.to_lowercase().contains(&kw)
                    || entry.description.to_lowercase().contains(&kw)
                {
                    Some(entry)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Return all entries in insertion order.
    pub fn list_all(&self) -> Vec<&IndexEntry> {
        self.order
            .iter()
            .filter_map(|name| self.entries.get(name))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Derive the default index path from a repository root.
    pub fn default_index_path(repo_root: &Path) -> PathBuf {
        repo_root.join(INDEX_FILENAME)
    }

    /// Derive the default hash path from a repository root.
    pub fn default_hash_path(repo_root: &Path) -> PathBuf {
        repo_root.join(INDEX_HASH_FILENAME)
    }
}

/// Build an IndexEntry from a .prot package's built-in metadata.
/// This extracts the necessary fields from the package's META/ contents.
#[allow(clippy::too_many_arguments)]
pub fn build_index_entry(
    name: &str,
    version: &str,
    release: u32,
    arch: &str,
    category: &str,
    filename: &str,
    sha256: &str,
    description: &str,
    license: &str,
    depends_runtime: Vec<String>,
    depends_build: Vec<String>,
    installed_size: u64,
) -> IndexEntry {
    let now = chrono::Utc::now();
    IndexEntry {
        name: name.to_string(),
        version: version.to_string(),
        release,
        description: description.to_string(),
        license: license.to_string(),
        arch: arch.to_string(),
        category: category.to_string(),
        filename: filename.to_string(),
        sha256: sha256.to_string(),
        depends: IndexDepends {
            runtime: depends_runtime,
            build: depends_build,
        },
        provides: vec![],
        conflicts: vec![],
        installed_size,
        build_date: now.to_rfc3339(),
    }
}

fn sha256_str(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    format!("{result:x}")
}
