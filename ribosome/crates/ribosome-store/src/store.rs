use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::digest::Sha256Digest;
use crate::error::{Result, StoreError};

/// A handle to an object stored in the vacuole CAS.
///
/// The object is an on-disk file; this handle holds its path and digest.
pub struct ObjectHandle {
    path: PathBuf,
    digest: Sha256Digest,
}

impl ObjectHandle {
    /// The content digest of this object.
    pub fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    /// Path to the object file on disk.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Open a read handle to the object contents.
    pub fn open(&self) -> Result<std::fs::File> {
        std::fs::File::open(&self.path).map_err(|e| StoreError::io(&self.path, e.to_string()))
    }

    /// Read the entire object into a byte vector.
    pub fn read_bytes(&self) -> Result<Vec<u8>> {
        std::fs::read(&self.path).map_err(|e| StoreError::io(&self.path, e.to_string()))
    }

    /// Object size in bytes.
    pub fn size(&self) -> Result<u64> {
        std::fs::metadata(&self.path)
            .map(|m| m.len())
            .map_err(|e| StoreError::io(&self.path, e.to_string()))
    }
}

/// The vacuole content-addressable store.
///
/// Stores arbitrary blobs keyed by their SHA-256 digest in a Git-style
/// sharded directory layout. Each blob is written atomically via
/// a temporary file + rename.
///
/// # Layout
///
/// ```text
/// <root>/
/// ├── objects/
/// │   ├── ab/
/// │   │   └── cdef1234...
/// │   └── ...
/// └── refs/
///     ├── packages/
///     └── sources/
/// ```
pub struct VacuoleStore {
    pub(crate) root: PathBuf,
}

impl VacuoleStore {
    /// Open (or create) a vacuole store at the given root directory.
    pub fn open(root: &Path) -> Result<Self> {
        let objects_dir = root.join("objects");
        let refs_dir = root.join("refs");

        std::fs::create_dir_all(&objects_dir)
            .map_err(|e| StoreError::io(&objects_dir, e.to_string()))?;
        std::fs::create_dir_all(&refs_dir).map_err(|e| StoreError::io(&refs_dir, e.to_string()))?;

        info!(root = %root.display(), "opened vacuole store");
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// The root directory of this store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ------------------------------------------------------------------
    // Write operations
    // ------------------------------------------------------------------

    /// Store a byte slice, returning its content digest.
    ///
    /// Idempotent: if the object already exists, returns immediately.
    pub fn put_bytes(&self, data: &[u8]) -> Result<Sha256Digest> {
        let digest = Sha256Digest::from_bytes(data);

        if self.contains(&digest)? {
            debug!(hash = %digest, "object already cached");
            return Ok(digest);
        }

        let obj_path = digest.object_path(&self.root);
        let shard_dir = obj_path.parent().unwrap_or_else(|| {
            panic!(
                "object path should always have a parent: {}",
                obj_path.display()
            )
        });
        std::fs::create_dir_all(shard_dir).map_err(|e| StoreError::io(shard_dir, e.to_string()))?;

        write_atomic(&obj_path, data)?;
        debug!(hash = %digest, size = data.len(), "stored object");
        Ok(digest)
    }

    /// Store a file by path, streaming it through the hasher without
    /// loading the entire file into memory.
    ///
    /// Idempotent: if the object already exists, returns immediately.
    /// The source file is **not** deleted after storing.
    pub fn put_file(&self, path: &Path) -> Result<Sha256Digest> {
        let mut src = std::fs::File::open(path).map_err(|e| StoreError::io(path, e.to_string()))?;

        let metadata = src
            .metadata()
            .map_err(|e| StoreError::io(path, e.to_string()))?;
        let file_size = metadata.len();

        // Phase 1: hash the file to determine the digest
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024]; // 64 KiB read buffer
        loop {
            let n = src
                .read(&mut buf)
                .map_err(|e| StoreError::io(path, e.to_string()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let digest = Sha256Digest::from_bytes_raw(hasher.finalize().into());

        if self.contains(&digest)? {
            debug!(hash = %digest, "file object already cached");
            return Ok(digest);
        }

        // Phase 2: write to CAS, hashing the written stream to detect TOCTOU
        let obj_path = digest.object_path(&self.root);
        let shard_dir = obj_path.parent().unwrap();
        std::fs::create_dir_all(shard_dir).map_err(|e| StoreError::io(shard_dir, e.to_string()))?;

        // Re-read the source and write to the temp file, verifying hash along the way
        src.seek(std::io::SeekFrom::Start(0))
            .map_err(|e| StoreError::io(path, e.to_string()))?;

        let tmp_path = obj_path.with_extension(format!("tmp.{}", std::process::id()));
        let verify_digest = {
            let mut dst = std::fs::File::create(&tmp_path)
                .map_err(|e| StoreError::io(&tmp_path, e.to_string()))?;
            let mut verify_hasher = Sha256::new();
            loop {
                let n = src
                    .read(&mut buf)
                    .map_err(|e| StoreError::io(path, e.to_string()))?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n])
                    .map_err(|e| StoreError::io(&tmp_path, e.to_string()))?;
                verify_hasher.update(&buf[..n]);
            }
            dst.sync_all()
                .map_err(|e| StoreError::io(&tmp_path, e.to_string()))?;
            Sha256Digest::from_bytes_raw(verify_hasher.finalize().into())
        };

        // Verify the content we actually wrote matches the expected digest
        if verify_digest != digest {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(StoreError::HashMismatch {
                expected: digest.hex(),
                actual: verify_digest.hex(),
            });
        }

        // Rename is atomic on POSIX when src and dst are on the same mount point
        match std::fs::rename(&tmp_path, &obj_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Race: another process wrote the same object
                let _ = std::fs::remove_file(&tmp_path);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(StoreError::io(
                    &obj_path,
                    format!("failed to rename tmp to object: {e}"),
                ));
            }
        }

        debug!(hash = %digest, size = file_size, "stored file object");
        Ok(digest)
    }

    // ------------------------------------------------------------------
    // Read operations
    // ------------------------------------------------------------------

    /// Check whether an object exists in the store.
    pub fn contains(&self, digest: &Sha256Digest) -> Result<bool> {
        let path = digest.object_path(&self.root);
        Ok(path.exists())
    }

    /// Get a handle to an object, if it exists.
    pub fn get(&self, digest: &Sha256Digest) -> Result<Option<ObjectHandle>> {
        let path = digest.object_path(&self.root);
        if path.exists() {
            Ok(Some(ObjectHandle {
                path,
                digest: digest.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the size of an object in bytes, if it exists.
    pub fn size(&self, digest: &Sha256Digest) -> Result<Option<u64>> {
        let path = digest.object_path(&self.root);
        if path.exists() {
            let size = std::fs::metadata(&path)
                .map(|m| m.len())
                .map_err(|e| StoreError::io(&path, e.to_string()))?;
            Ok(Some(size))
        } else {
            Ok(None)
        }
    }

    // ------------------------------------------------------------------
    // Statistics
    // ------------------------------------------------------------------

    /// Count the total number of objects and total bytes on disk.
    pub fn stats(&self) -> Result<StoreStats> {
        let objects_dir = self.root.join("objects");
        let mut count = 0u64;
        let mut total_size = 0u64;

        if objects_dir.exists() {
            for entry in walkdir_entries(&objects_dir)? {
                if entry.file_type().is_ok_and(|ft| ft.is_file()) {
                    count += 1;
                    total_size += entry.metadata().map_or(0, |m| m.len());
                }
            }
        }

        Ok(StoreStats {
            object_count: count,
            total_bytes: total_size,
        })
    }
}

/// Store-wide statistics.
#[derive(Debug)]
pub struct StoreStats {
    pub object_count: u64,
    pub total_bytes: u64,
}

// ------------------------------------------------------------------
// Internal helpers
// ------------------------------------------------------------------

/// Write data to `path` atomically via a temp file + rename.
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));

    {
        let mut f = std::fs::File::create(&tmp_path)
            .map_err(|e| StoreError::io(&tmp_path, e.to_string()))?;
        f.write_all(data)
            .map_err(|e| StoreError::io(&tmp_path, e.to_string()))?;
        f.sync_all()
            .map_err(|e| StoreError::io(&tmp_path, e.to_string()))?;
    }

    match std::fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Race: another process wrote the same object
            let _ = std::fs::remove_file(&tmp_path);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(StoreError::io(
                path,
                format!("failed to rename tmp to object: {e}"),
            ))
        }
    }
}

/// Walk a directory recursively, yielding all entries (non-recursive depth
/// is fine since we only have 2 levels under objects/).
fn walkdir_entries(dir: &Path) -> Result<Vec<std::fs::DirEntry>> {
    let mut entries = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let read_dir =
            std::fs::read_dir(&current).map_err(|e| StoreError::io(&current, e.to_string()))?;
        for entry in read_dir {
            let entry = entry.map_err(|e| StoreError::io(&current, e.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|e| StoreError::io(entry.path(), e.to_string()))?;
            if file_type.is_dir() {
                stack.push(entry.path());
            } else {
                entries.push(entry);
            }
        }
    }

    Ok(entries)
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
    fn put_bytes_stores_object() {
        let (_tmp, store) = make_store();
        let digest = store.put_bytes(b"hello world").unwrap();

        assert!(store.contains(&digest).unwrap());
        let handle = store.get(&digest).unwrap().unwrap();
        assert_eq!(handle.read_bytes().unwrap(), b"hello world");
    }

    #[test]
    fn put_bytes_is_idempotent() {
        let (_tmp, store) = make_store();
        let d1 = store.put_bytes(b"data").unwrap();
        let d2 = store.put_bytes(b"data").unwrap();
        assert_eq!(d1, d2);
        assert_eq!(store.stats().unwrap().object_count, 1);
    }

    #[test]
    fn put_bytes_different_content_different_digest() {
        let (_tmp, store) = make_store();
        let d1 = store.put_bytes(b"aaa").unwrap();
        let d2 = store.put_bytes(b"bbb").unwrap();
        assert_ne!(d1, d2);
        assert_eq!(store.stats().unwrap().object_count, 2);
    }

    #[test]
    fn put_file_stores_file() {
        let (_tmp, store) = make_store();

        let file_dir = tempfile::tempdir().unwrap();
        let file_path = file_dir.path().join("test.bin");
        std::fs::write(&file_path, b"file content").unwrap();

        let digest = store.put_file(&file_path).unwrap();
        assert!(store.contains(&digest).unwrap());

        let handle = store.get(&digest).unwrap().unwrap();
        assert_eq!(handle.read_bytes().unwrap(), b"file content");
    }

    #[test]
    fn put_file_is_idempotent() {
        let (_tmp, store) = make_store();

        let file_dir = tempfile::tempdir().unwrap();
        let file_path = file_dir.path().join("test.bin");
        std::fs::write(&file_path, b"unique data").unwrap();

        let d1 = store.put_file(&file_path).unwrap();
        let d2 = store.put_file(&file_path).unwrap();
        assert_eq!(d1, d2);
        assert_eq!(store.stats().unwrap().object_count, 1);
    }

    #[test]
    fn put_file_and_put_bytes_same_content_same_digest() {
        let (_tmp, store) = make_store();

        let d1 = store.put_bytes(b"shared content").unwrap();

        let file_dir = tempfile::tempdir().unwrap();
        let file_path = file_dir.path().join("test.bin");
        std::fs::write(&file_path, b"shared content").unwrap();
        let d2 = store.put_file(&file_path).unwrap();

        assert_eq!(d1, d2);
        assert_eq!(store.stats().unwrap().object_count, 1);
    }

    #[test]
    fn get_returns_none_for_missing() {
        let (_tmp, store) = make_store();
        let digest = Sha256Digest::from_bytes(b"nonexistent");
        assert!(store.get(&digest).unwrap().is_none());
        assert!(!store.contains(&digest).unwrap());
    }

    #[test]
    fn size_returns_correct_bytes() {
        let (_tmp, store) = make_store();
        let data = b"12345";
        let digest = store.put_bytes(data).unwrap();
        assert_eq!(store.size(&digest).unwrap(), Some(5));
    }

    #[test]
    fn size_returns_none_for_missing() {
        let (_tmp, store) = make_store();
        let digest = Sha256Digest::from_bytes(b"missing");
        assert!(store.size(&digest).unwrap().is_none());
    }

    #[test]
    fn stats_counts_multiple_objects() {
        let (_tmp, store) = make_store();
        store.put_bytes(b"one").unwrap();
        store.put_bytes(b"two").unwrap();
        store.put_bytes(b"three").unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.object_count, 3);
        assert_eq!(stats.total_bytes, 3 + 3 + 5); // "one" + "two" + "three"
    }

    #[test]
    fn put_file_does_not_delete_source() {
        let (_tmp, store) = make_store();

        let file_dir = tempfile::tempdir().unwrap();
        let file_path = file_dir.path().join("persist.bin");
        std::fs::write(&file_path, b"keep me").unwrap();

        store.put_file(&file_path).unwrap();
        assert!(file_path.exists(), "source file should not be deleted");
    }

    #[test]
    fn shard_directory_created_on_demand() {
        let (_tmp, store) = make_store();
        let digest = store.put_bytes(b"create shard dir").unwrap();

        let shard_dir = store.root.join("objects").join(digest.shard());
        assert!(shard_dir.is_dir());
    }
}
