//! Btrfs subvolume operations for fast build directory management.
//!
//! When the build directory resides on a Btrfs filesystem, subvolumes provide
//! O(1) creation and deletion compared to recursive directory operations.
//! This is especially valuable for large build trees with many source files.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use tracing::{debug, info, warn};

use crate::error::{Result, SandboxError};

/// Check if a path resides on a Btrfs filesystem.
///
/// Uses `statfs` via the `btrfs` filesystem type magic number (`0x9123683E`).
/// Falls back to running `btrfs filesystem df` if the direct check is not available.
pub fn is_btrfs(path: &Path) -> bool {
    // Try using statfs via libc
    #[cfg(target_os = "linux")]
    {
        // SAFETY: libc::statfs requires a valid out-pointer to a statfs struct.
        // We zero-initialize via zeroed() which is valid for C structs where the
        // kernel fills in fields. c_path is a valid CString live for the call duration.
        let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
        let c_path = match std::ffi::CString::new(path.as_os_str().as_bytes()) {
            Ok(p) => p,
            Err(_) => return false,
        };
        // SAFETY: c_path.as_ptr() is valid and NUL-terminated; buf is a valid out-pointer.
        let result = unsafe { libc::statfs(c_path.as_ptr(), &mut buf) };
        if result == 0 {
            // Btrfs super magic: 0x9123683E
            const BTRFS_SUPER_MAGIC: i64 = 0x9123_683E;
            return buf.f_type == BTRFS_SUPER_MAGIC;
        }
    }

    // Fallback: try `btrfs filesystem df`
    let output = std::process::Command::new("btrfs")
        .args(["filesystem", "df"])
        .arg(path)
        .output();

    matches!(output, Ok(output) if output.status.success())
}

/// Check if a path is already a Btrfs subvolume.
///
/// Runs `btrfs subvolume show <path>` and returns true if it succeeds.
pub fn is_subvolume(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    let output = std::process::Command::new("btrfs")
        .args(["subvolume", "show"])
        .arg(path)
        .output();

    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Create a Btrfs subvolume at the given path.
///
/// Uses `btrfs subvolume create <path>`. Requires root or `CAP_SYS_ADMIN`.
pub fn create_subvolume(path: &Path) -> Result<()> {
    info!(path = %path.display(), "creating Btrfs subvolume");

    let output = std::process::Command::new("btrfs")
        .args(["subvolume", "create"])
        .arg(path)
        .output()
        .map_err(|e| {
            SandboxError::CreationFailed(format!("failed to run btrfs subvolume create: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SandboxError::CreationFailed(format!(
            "btrfs subvolume create failed for {}: {stderr}",
            path.display()
        )));
    }

    Ok(())
}

/// Delete a Btrfs subvolume at the given path.
///
/// Uses `btrfs subvolume delete <path>`. Requires root or `CAP_SYS_ADMIN`.
pub fn delete_subvolume(path: &Path) -> Result<()> {
    info!(path = %path.display(), "deleting Btrfs subvolume");

    let output = std::process::Command::new("btrfs")
        .args(["subvolume", "delete"])
        .arg(path)
        .output()
        .map_err(|e| {
            SandboxError::CreationFailed(format!("failed to run btrfs subvolume delete: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SandboxError::CreationFailed(format!(
            "btrfs subvolume delete failed for {}: {stderr}",
            path.display()
        )));
    }

    Ok(())
}

/// Create a read-write Btrfs snapshot from a source subvolume.
///
/// Uses `btrfs subvolume snapshot <source> <target>`.
pub fn create_snapshot(source: &Path, target: &Path) -> Result<()> {
    info!(
        source = %source.display(),
        target = %target.display(),
        "creating Btrfs snapshot"
    );

    let output = std::process::Command::new("btrfs")
        .args(["subvolume", "snapshot"])
        .arg(source)
        .arg(target)
        .output()
        .map_err(|e| {
            SandboxError::CreationFailed(format!("failed to run btrfs subvolume snapshot: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SandboxError::CreationFailed(format!(
            "btrfs snapshot failed: {} -> {}: {stderr}",
            source.display(),
            target.display()
        )));
    }

    Ok(())
}

/// Create a build directory, preferring Btrfs subvolume when available.
///
/// If the parent directory is on a Btrfs filesystem and `btrfs` command
/// is available, creates a subvolume. Otherwise falls back to `mkdir`.
pub fn create_build_dir(path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or(path);

    if parent == path {
        // Root-level path, skip subvolume detection
    } else if is_btrfs(parent) && !path.exists() {
        debug!(path = %path.display(), "Btrfs detected, creating subvolume");
        match create_subvolume(path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!(error = %e, "Btrfs subvolume creation failed, falling back to mkdir");
            }
        }
    }

    // Fallback to regular directory creation
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(|e| {
            SandboxError::CreationFailed(format!(
                "failed to create directory {}: {e}",
                path.display()
            ))
        })?;
    }

    Ok(())
}

/// Remove a build directory, using Btrfs subvolume delete when applicable.
///
/// If the path is a Btrfs subvolume, uses `btrfs subvolume delete` for
/// instantaneous cleanup. Otherwise falls back to `rm -rf`.
pub fn remove_build_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if is_subvolume(path) {
        debug!(path = %path.display(), "Btrfs subvolume detected, using subvolume delete");
        match delete_subvolume(path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!(error = %e, "Btrfs subvolume delete failed, falling back to rm -rf");
            }
        }
    }

    // Fallback to recursive removal
    std::fs::remove_dir_all(path).map_err(|e| {
        SandboxError::CreationFailed(format!(
            "failed to remove directory {}: {e}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_btrfs_on_nonexistent_path() {
        let result = is_btrfs(Path::new("/definitely/not/a/real/path"));
        assert!(!result, "nonexistent path should not be reported as Btrfs");
    }

    #[test]
    fn test_is_subvolume_on_nonexistent_path() {
        assert!(!is_subvolume(Path::new("/definitely/not/a/real/path")));
    }

    #[test]
    fn test_is_subvolume_on_regular_dir() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        assert!(!is_subvolume(tmp.path()));
    }

    #[test]
    fn test_create_build_dir_regular() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let build_dir = tmp.path().join("test-build");

        create_build_dir(&build_dir).expect("should create dir");

        assert!(build_dir.is_dir());
    }

    #[test]
    fn test_create_build_dir_idempotent() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let build_dir = tmp.path().join("test-build");

        create_build_dir(&build_dir).expect("first create");
        create_build_dir(&build_dir).expect("second create");

        assert!(build_dir.is_dir());
    }

    #[test]
    fn test_remove_build_dir() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let build_dir = tmp.path().join("test-build");

        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::write(build_dir.join("test.txt"), "hello").unwrap();

        remove_build_dir(&build_dir).expect("should remove dir");
        assert!(!build_dir.exists());
    }

    #[test]
    fn test_remove_build_dir_nonexistent() {
        let result = remove_build_dir(Path::new("/tmp/nonexistent_test_dir_xyz"));
        assert!(result.is_ok(), "removing nonexistent dir should succeed");
    }
}
