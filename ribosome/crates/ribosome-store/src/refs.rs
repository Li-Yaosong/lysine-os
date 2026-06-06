use std::path::PathBuf;

use tracing::debug;

use crate::digest::Sha256Digest;
use crate::error::{Result, StoreError};
use crate::store::VacuoleStore;

/// Reference namespace constants.
pub const NS_PACKAGES: &str = "packages";
pub const NS_SOURCES: &str = "sources";

impl VacuoleStore {
    /// Create or update a named reference pointing to a digest.
    ///
    /// Refs act as GC roots -- any object reachable through a ref is
    /// protected from garbage collection.
    ///
    /// If the ref already exists it is overwritten.
    pub fn add_ref(&self, namespace: &str, name: &str, digest: &Sha256Digest) -> Result<()> {
        let ref_path = self.ref_path(namespace, name);
        if let Some(parent) = ref_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::io(parent, e.to_string()))?;
        }
        std::fs::write(&ref_path, digest.hex())
            .map_err(|e| StoreError::io(&ref_path, e.to_string()))?;
        debug!(namespace, name, hash = %digest, "added ref");
        Ok(())
    }

    /// Resolve a named reference to its digest, if the ref exists.
    pub fn resolve_ref(&self, namespace: &str, name: &str) -> Result<Option<Sha256Digest>> {
        let ref_path = self.ref_path(namespace, name);
        if !ref_path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&ref_path)
            .map_err(|e| StoreError::io(&ref_path, e.to_string()))?;
        let digest = Sha256Digest::parse(content.trim())?;
        Ok(Some(digest))
    }

    /// List all refs under a namespace, returning `(name, digest)` pairs.
    pub fn list_refs(&self, namespace: &str) -> Result<Vec<(String, Sha256Digest)>> {
        let ns_dir = self.root().join("refs").join(namespace);
        if !ns_dir.exists() {
            return Ok(Vec::new());
        }

        let mut refs = Vec::new();
        for entry in
            std::fs::read_dir(&ns_dir).map_err(|e| StoreError::io(&ns_dir, e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::io(&ns_dir, e.to_string()))?;
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let content = std::fs::read_to_string(entry.path())
                .map_err(|e| StoreError::io(entry.path(), e.to_string()))?;
            match Sha256Digest::parse(content.trim()) {
                Ok(digest) => refs.push((name, digest)),
                Err(e) => {
                    // Skip malformed refs but log a warning
                    debug!(name = %name, error = %e, "skipping malformed ref");
                }
            }
        }
        refs.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(refs)
    }

    /// Remove a named reference.
    ///
    /// This does **not** delete the referenced object; call `gc()` to
    /// reclaim unreferenced objects.
    pub fn remove_ref(&self, namespace: &str, name: &str) -> Result<()> {
        let ref_path = self.ref_path(namespace, name);
        if ref_path.exists() {
            std::fs::remove_file(&ref_path)
                .map_err(|e| StoreError::io(&ref_path, e.to_string()))?;
            debug!(namespace, name, "removed ref");
        }
        Ok(())
    }

    /// Check whether a named reference exists.
    pub fn has_ref(&self, namespace: &str, name: &str) -> Result<bool> {
        Ok(self.ref_path(namespace, name).exists())
    }

    // ------------------------------------------------------------------
    // Convenience shortcuts for common namespaces
    // ------------------------------------------------------------------

    /// Store a package reference: `refs/packages/<name>-<ver>-<rel>-<arch>`.
    pub fn add_package_ref(
        &self,
        name: &str,
        version: &str,
        release: u32,
        arch: &str,
        digest: &Sha256Digest,
    ) -> Result<()> {
        let ref_name = format!("{name}-{version}-{release}-{arch}");
        self.add_ref(NS_PACKAGES, &ref_name, digest)
    }

    /// Resolve a package reference.
    pub fn resolve_package_ref(
        &self,
        name: &str,
        version: &str,
        release: u32,
        arch: &str,
    ) -> Result<Option<Sha256Digest>> {
        let ref_name = format!("{name}-{version}-{release}-{arch}");
        self.resolve_ref(NS_PACKAGES, &ref_name)
    }

    /// Store a source tarball reference.
    pub fn add_source_ref(&self, filename: &str, digest: &Sha256Digest) -> Result<()> {
        self.add_ref(NS_SOURCES, filename, digest)
    }

    /// Resolve a source tarball reference.
    pub fn resolve_source_ref(&self, filename: &str) -> Result<Option<Sha256Digest>> {
        self.resolve_ref(NS_SOURCES, filename)
    }

    // ------------------------------------------------------------------
    // Internal
    // ------------------------------------------------------------------

    fn ref_path(&self, namespace: &str, name: &str) -> PathBuf {
        self.root().join("refs").join(namespace).join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> (tempfile::TempDir, VacuoleStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = VacuoleStore::open(tmp.path()).unwrap();
        (tmp, store)
    }

    #[test]
    fn add_and_resolve_ref() {
        let (_tmp, store) = make_store();
        let digest = store.put_bytes(b"test data").unwrap();

        store.add_ref("test", "my-obj", &digest).unwrap();

        let resolved = store.resolve_ref("test", "my-obj").unwrap();
        assert_eq!(resolved, Some(digest));
    }

    #[test]
    fn resolve_missing_ref_returns_none() {
        let (_tmp, store) = make_store();
        assert!(store.resolve_ref("test", "nope").unwrap().is_none());
    }

    #[test]
    fn remove_ref() {
        let (_tmp, store) = make_store();
        let digest = store.put_bytes(b"x").unwrap();
        store.add_ref("test", "rm-me", &digest).unwrap();
        assert!(store.has_ref("test", "rm-me").unwrap());

        store.remove_ref("test", "rm-me").unwrap();
        assert!(!store.has_ref("test", "rm-me").unwrap());
    }

    #[test]
    fn remove_missing_ref_is_ok() {
        let (_tmp, store) = make_store();
        store.remove_ref("test", "ghost").unwrap();
    }

    #[test]
    fn list_refs_returns_sorted() {
        let (_tmp, store) = make_store();
        let d1 = store.put_bytes(b"a").unwrap();
        let d2 = store.put_bytes(b"b").unwrap();
        let d3 = store.put_bytes(b"c").unwrap();

        store.add_ref("test", "charlie", &d3).unwrap();
        store.add_ref("test", "alpha", &d1).unwrap();
        store.add_ref("test", "bravo", &d2).unwrap();

        let refs = store.list_refs("test").unwrap();
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].0, "alpha");
        assert_eq!(refs[1].0, "bravo");
        assert_eq!(refs[2].0, "charlie");
    }

    #[test]
    fn list_refs_empty_namespace() {
        let (_tmp, store) = make_store();
        let refs = store.list_refs("nonexistent").unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn add_ref_overwrites_existing() {
        let (_tmp, store) = make_store();
        let d1 = store.put_bytes(b"old").unwrap();
        let d2 = store.put_bytes(b"new").unwrap();

        store.add_ref("test", "key", &d1).unwrap();
        store.add_ref("test", "key", &d2).unwrap();

        let resolved = store.resolve_ref("test", "key").unwrap();
        assert_eq!(resolved, Some(d2));
    }

    #[test]
    fn package_ref_shortcut() {
        let (_tmp, store) = make_store();
        let digest = store.put_bytes(b"gcc package").unwrap();

        store
            .add_package_ref("gcc", "14.2.0", 1, "x86_64", &digest)
            .unwrap();

        let resolved = store
            .resolve_package_ref("gcc", "14.2.0", 1, "x86_64")
            .unwrap();
        assert_eq!(resolved, Some(digest));
    }

    #[test]
    fn source_ref_shortcut() {
        let (_tmp, store) = make_store();
        let digest = store.put_bytes(b"tarball data").unwrap();

        store.add_source_ref("gcc-14.2.0.tar.xz", &digest).unwrap();

        let resolved = store.resolve_source_ref("gcc-14.2.0.tar.xz").unwrap();
        assert_eq!(resolved, Some(digest));
    }
}
