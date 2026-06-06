use std::collections::HashSet;

use tracing::{debug, info};

use crate::digest::Sha256Digest;
use crate::error::{Result, StoreError};
use crate::store::VacuoleStore;

/// Statistics returned by a garbage collection pass.
#[derive(Debug)]
pub struct GcStats {
    /// Objects that were reachable (kept alive by refs).
    pub reachable: u64,
    /// Objects that were unreachable and deleted.
    pub removed: u64,
    /// Bytes reclaimed by removing unreachable objects.
    pub bytes_reclaimed: u64,
}

impl VacuoleStore {
    /// Run garbage collection: delete all objects not reachable from any ref.
    ///
    /// Algorithm:
    /// 1. Walk all `refs/` directories, collect referenced digests
    /// 2. Walk all `objects/` shards
    /// 3. Delete any object whose digest is not in the reachable set
    pub fn gc(&self) -> Result<GcStats> {
        info!("starting vacuole GC");

        let reachable_digests = self.collect_reachable()?;
        debug!(count = reachable_digests.len(), "collected reachable refs");

        let objects_dir = self.root().join("objects");
        if !objects_dir.exists() {
            return Ok(GcStats {
                reachable: 0,
                removed: 0,
                bytes_reclaimed: 0,
            });
        }

        let mut removed = 0u64;
        let mut bytes_reclaimed = 0u64;
        let mut total_reachable = 0u64;

        // Walk shard directories (e.g. objects/ab/, objects/3e/, ...)
        let shard_dirs = std::fs::read_dir(&objects_dir)
            .map_err(|e| StoreError::io(&objects_dir, e.to_string()))?;

        for shard_entry in shard_dirs {
            let shard_entry =
                shard_entry.map_err(|e| StoreError::io(&objects_dir, e.to_string()))?;
            let shard_path = shard_entry.path();
            if !shard_path.is_dir() {
                continue;
            }

            let shard_name = shard_entry.file_name().to_string_lossy().into_owned();
            // Validate shard format: exactly 2 hex chars
            if shard_name.len() != 2 || !shard_name.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }

            let object_entries = std::fs::read_dir(&shard_path)
                .map_err(|e| StoreError::io(&shard_path, e.to_string()))?;

            for obj_entry in object_entries {
                let obj_entry =
                    obj_entry.map_err(|e| StoreError::io(&shard_path, e.to_string()))?;
                let obj_path = obj_entry.path();

                // Skip temp files
                let file_name = obj_entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                if file_name_str.contains(".tmp") {
                    debug!(path = %obj_path.display(), "removing stale tmp file");
                    let _ = std::fs::remove_file(&obj_path);
                    continue;
                }

                // Reconstruct the full hex digest from shard + file name
                let full_hex = format!("{shard_name}{file_name_str}");
                if full_hex.len() != 64 {
                    debug!(path = %obj_path.display(), "skipping non-digest file");
                    continue;
                }

                let Ok(digest) = Sha256Digest::parse(&full_hex) else {
                    debug!(path = %obj_path.display(), "skipping unparseable file name");
                    continue;
                };

                if reachable_digests.contains(&digest) {
                    total_reachable += 1;
                } else {
                    let size = std::fs::metadata(&obj_path).map_or(0, |m| m.len());
                    std::fs::remove_file(&obj_path).map_err(|e| {
                        StoreError::GcFailed(format!(
                            "failed to remove {}: {e}",
                            obj_path.display()
                        ))
                    })?;
                    debug!(hash = %digest, size, "removed unreachable object");
                    removed += 1;
                    bytes_reclaimed += size;
                }
            }
        }

        info!(
            reachable = total_reachable,
            removed, bytes_reclaimed, "GC complete"
        );

        Ok(GcStats {
            reachable: total_reachable,
            removed,
            bytes_reclaimed,
        })
    }

    /// Collect all digests reachable from any ref.
    fn collect_reachable(&self) -> Result<HashSet<Sha256Digest>> {
        let refs_dir = self.root().join("refs");
        let mut reachable = HashSet::new();

        if !refs_dir.exists() {
            return Ok(reachable);
        }

        let ns_entries =
            std::fs::read_dir(&refs_dir).map_err(|e| StoreError::io(&refs_dir, e.to_string()))?;

        for ns_entry in ns_entries {
            let ns_entry = ns_entry.map_err(|e| StoreError::io(&refs_dir, e.to_string()))?;
            if !ns_entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                continue;
            }
            let ns_dir = ns_entry.path();
            let ns_name = ns_entry.file_name().to_string_lossy().into_owned();

            let ref_entries =
                std::fs::read_dir(&ns_dir).map_err(|e| StoreError::io(&ns_dir, e.to_string()))?;

            for ref_entry in ref_entries {
                let ref_entry = ref_entry.map_err(|e| StoreError::io(&ns_dir, e.to_string()))?;
                if !ref_entry.file_type().is_ok_and(|ft| ft.is_file()) {
                    continue;
                }
                let content = std::fs::read_to_string(ref_entry.path())
                    .map_err(|e| StoreError::io(ref_entry.path(), e.to_string()))?;
                if let Ok(digest) = Sha256Digest::parse(content.trim()) {
                    reachable.insert(digest);
                } else {
                    debug!(
                        namespace = %ns_name,
                        ref_name = %ref_entry.file_name().to_string_lossy(),
                        "skipping ref with invalid digest"
                    );
                }
            }
        }

        Ok(reachable)
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
    fn gc_removes_unreferenced_objects() {
        let (_tmp, store) = make_store();

        let d1 = store.put_bytes(b"kept").unwrap();
        let d2 = store.put_bytes(b"removed").unwrap();

        // Only ref d1
        store.add_ref("test", "important", &d1).unwrap();

        let stats = store.gc().unwrap();
        assert_eq!(stats.reachable, 1);
        assert_eq!(stats.removed, 1);
        assert!(store.contains(&d1).unwrap());
        assert!(!store.contains(&d2).unwrap());
    }

    #[test]
    fn gc_keeps_all_referenced() {
        let (_tmp, store) = make_store();

        let d1 = store.put_bytes(b"pkg-a").unwrap();
        let d2 = store.put_bytes(b"pkg-b").unwrap();
        let d3 = store.put_bytes(b"source").unwrap();

        store.add_ref("packages", "a-1.0", &d1).unwrap();
        store.add_ref("packages", "b-2.0", &d2).unwrap();
        store.add_ref("sources", "src.tar.xz", &d3).unwrap();

        let stats = store.gc().unwrap();
        assert_eq!(stats.removed, 0);
        assert_eq!(stats.reachable, 3);

        assert!(store.contains(&d1).unwrap());
        assert!(store.contains(&d2).unwrap());
        assert!(store.contains(&d3).unwrap());
    }

    #[test]
    fn gc_on_empty_store() {
        let (_tmp, store) = make_store();
        let stats = store.gc().unwrap();
        assert_eq!(stats.reachable, 0);
        assert_eq!(stats.removed, 0);
    }

    #[test]
    fn gc_removes_orphaned_after_ref_removed() {
        let (_tmp, store) = make_store();

        let digest = store.put_bytes(b"orphan").unwrap();
        store.add_ref("test", "temp", &digest).unwrap();

        // Object is referenced
        let stats1 = store.gc().unwrap();
        assert_eq!(stats1.removed, 0);
        assert!(store.contains(&digest).unwrap());

        // Remove the ref
        store.remove_ref("test", "temp").unwrap();

        // Now GC should reclaim it
        let stats2 = store.gc().unwrap();
        assert_eq!(stats2.removed, 1);
        assert!(!store.contains(&digest).unwrap());
    }

    #[test]
    fn gc_cleans_up_temp_files() {
        let (_tmp, store) = make_store();

        // Manually create a stale tmp file
        let shard_dir = store.root().join("objects").join("ab");
        std::fs::create_dir_all(&shard_dir).unwrap();
        let tmp_file = shard_dir.join("something.tmp.12345");
        std::fs::write(&tmp_file, b"stale").unwrap();

        let stats = store.gc().unwrap();
        assert_eq!(stats.removed, 0);
        assert!(!tmp_file.exists(), "stale tmp file should be cleaned up");
    }

    #[test]
    fn gc_reclaims_bytes() {
        let (_tmp, store) = make_store();

        let _d_big = store.put_bytes(&vec![0u8; 1024]).unwrap();
        let d_small = store.put_bytes(b"tiny").unwrap();

        // Only ref the small one
        store.add_ref("test", "keep", &d_small).unwrap();

        let stats = store.gc().unwrap();
        assert_eq!(stats.removed, 1);
        assert_eq!(stats.bytes_reclaimed, 1024);
    }
}
