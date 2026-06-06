//! Minimal root filesystem management for membrane build sandboxes.
//!
//! A minimal rootfs provides an isolated filesystem for the build sandbox,
//! containing only the tools and libraries needed for compilation. This
//! prevents build scripts from accessing the host's full filesystem.
//!
//! # Layout
//!
//! ```text
//! /var/ribosome/rootfs/
//! ├── bin/         → sh, bash, coreutils
//! ├── lib/         → libc.so, ld-linux.so
//! ├── lib64/       → ld-linux-x86-64.so.2 (symlink)
//! ├── usr/bin/     → gcc, make, cmake, etc.
//! ├── usr/lib/     → toolchain libraries
//! ├── usr/include/ → system headers
//! ├── tmp/         → temporary files (empty)
//! ├── etc/         → minimal config (resolv.conf, passwd)
//! └── srv/         → build mount points (empty)
//! ```
//!
//! # Creation Strategy
//!
//! The rootfs is populated by copying binaries and their shared library
//! dependencies from the host system. This keeps it self-contained and
//! reproducible without requiring .prot packages to be pre-built.

use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::error::{Result, SandboxError};

/// Specification for the sandbox root filesystem.
#[derive(Debug, Clone, Default)]
pub enum RootfsSpec {
    /// Use the host root filesystem (default, least isolated).
    #[default]
    Host,
    /// Use a minimal build rootfs at the given path.
    Minimal {
        /// Path to the minimal rootfs directory.
        path: PathBuf,
    },
    /// Use an arbitrary custom rootfs path.
    Custom(PathBuf),
}

/// Manages a minimal root filesystem for build sandbox isolation.
pub struct MinimalRootfs {
    /// Root directory of the minimal rootfs.
    path: PathBuf,
}

impl MinimalRootfs {
    /// Create a new minimal rootfs manager at the given path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Get the rootfs path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create the minimal rootfs directory structure.
    ///
    /// Creates all required directories but does not populate them with
    /// binaries or libraries. Use `populate_from_host()` to install tools.
    pub fn create(&self) -> Result<()> {
        info!(path = %self.path.display(), "creating minimal rootfs directory structure");

        let dirs = [
            "bin",
            "sbin",
            "lib",
            "lib64",
            "usr/bin",
            "usr/sbin",
            "usr/lib",
            "usr/include",
            "usr/share",
            "tmp",
            "etc",
            "srv",
            "proc",
            "sys",
            "dev",
            "run",
            "var/cache",
            "var/tmp",
            "var/log",
        ];

        for dir in &dirs {
            let full_path = self.path.join(dir);
            if !full_path.exists() {
                std::fs::create_dir_all(&full_path).map_err(|e| {
                    SandboxError::CreationFailed(format!(
                        "failed to create rootfs directory {}: {e}",
                        full_path.display()
                    ))
                })?;
            }
        }

        Ok(())
    }

    /// Populate the rootfs by copying essential tools and their dependencies
    /// from the host system.
    ///
    /// This copies the specified binaries and resolves their shared library
    /// dependencies using `ldd`, copying all required `.so` files.
    pub fn populate_from_host(&self, tools: &[&str]) -> Result<PopulateReport> {
        info!(
            path = %self.path.display(),
            tools = tools.len(),
            "populating minimal rootfs from host"
        );

        let mut report = PopulateReport::default();

        for tool_name in tools {
            match self.install_tool_from_host(tool_name) {
                Ok(info) => {
                    debug!(
                        tool = tool_name,
                        binary = %info.binary_path.display(),
                        libs = info.libs.len(),
                        "installed tool"
                    );
                    report.tools_installed += 1;
                    report.libs_copied += info.libs.len();
                    report.files_copied += 1 + info.libs.len();
                }
                Err(e) => {
                    warn!(tool = tool_name, error = %e, "failed to install tool, skipping");
                    report.tools_skipped += 1;
                }
            }
        }

        // Create essential symlinks and config files
        self.create_essential_files()?;

        info!(
            tools_installed = report.tools_installed,
            tools_skipped = report.tools_skipped,
            libs_copied = report.libs_copied,
            "rootfs population complete"
        );

        Ok(report)
    }

    /// Install a single tool by finding it on PATH, copying it to the rootfs,
    /// and copying all its shared library dependencies.
    fn install_tool_from_host(&self, tool_name: &str) -> Result<ToolInstallInfo> {
        // Find the binary on the host system
        let binary_path = which(tool_name).ok_or_else(|| {
            SandboxError::CreationFailed(format!("tool '{tool_name}' not found on host PATH"))
        })?;

        // Determine where to place it in the rootfs
        let relative = binary_path.strip_prefix("/").unwrap_or(&binary_path);
        let dest = self.path.join(relative);

        // Copy the binary
        copy_file(&binary_path, &dest)?;

        // Resolve and copy shared library dependencies
        let libs = resolve_libs(&binary_path)?;
        let mut copied_libs = Vec::new();
        for lib_path in &libs {
            let lib_relative = lib_path.strip_prefix("/").unwrap_or(lib_path);
            let lib_dest = self.path.join(lib_relative);
            if let Some(parent) = lib_dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    SandboxError::CreationFailed(format!(
                        "failed to create lib directory {}: {e}",
                        parent.display()
                    ))
                })?;
            }
            match copy_file(lib_path, &lib_dest) {
                Ok(()) => copied_libs.push(lib_dest),
                Err(e) => {
                    warn!(
                        lib = %lib_path.display(),
                        error = %e,
                        "failed to copy shared library"
                    );
                }
            }
        }

        Ok(ToolInstallInfo {
            binary_path,
            libs: copied_libs,
        })
    }

    /// Create essential files in the rootfs (passwd, group, resolv.conf, etc.).
    fn create_essential_files(&self) -> Result<()> {
        let etc = self.path.join("etc");

        // /etc/passwd — root and nobody
        let passwd = "root:x:0:0:root:/root:/bin/bash\nnobody:x:65534:65534:Nobody:/nonexistent:/usr/sbin/nologin\n";
        std::fs::write(etc.join("passwd"), passwd).map_err(|e| {
            SandboxError::CreationFailed(format!("failed to write etc/passwd: {e}"))
        })?;

        // /etc/group — root and nobody
        let group = "root:x:0:\nnobody:x:65534:\n";
        std::fs::write(etc.join("group"), group)
            .map_err(|e| SandboxError::CreationFailed(format!("failed to write etc/group: {e}")))?;

        // Copy /etc/resolv.conf if it exists (for DNS resolution in non-isolated mode)
        let host_resolv = Path::new("/etc/resolv.conf");
        if host_resolv.exists() {
            if let Err(e) = std::fs::copy(host_resolv, etc.join("resolv.conf")) {
                warn!(error = %e, "failed to copy resolv.conf, DNS may not work");
            }
        }

        // Create /etc/hosts with minimal entries
        let hosts = "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n";
        std::fs::write(etc.join("hosts"), hosts)
            .map_err(|e| SandboxError::CreationFailed(format!("failed to write etc/hosts: {e}")))?;

        Ok(())
    }

    /// Verify the rootfs contains essential tools and directories.
    pub fn verify(&self) -> Result<VerifyReport> {
        let mut report = VerifyReport::default();

        // Check essential directories
        let required_dirs = ["bin", "lib", "usr/bin", "tmp", "etc"];
        for dir in &required_dirs {
            let full = self.path.join(dir);
            if full.is_dir() {
                report.dirs_ok += 1;
            } else {
                report.missing_dirs.push(dir.to_string());
            }
        }

        // Check essential binaries
        let required_tools = ["sh", "bash", "ls", "cat", "mkdir", "cp", "mv", "rm"];
        for tool in &required_tools {
            let found = self.find_in_rootfs(tool);
            if found.is_some() {
                report.tools_ok += 1;
            } else {
                report.missing_tools.push(tool.to_string());
            }
        }

        // Check C library
        let libc_paths = [
            self.path.join("lib/x86_64-linux-gnu/libc.so.6"),
            self.path.join("lib/libc.so.6"),
            self.path.join("lib64/libc.so.6"),
        ];
        report.has_libc = libc_paths.iter().any(|p| p.exists());

        // Check dynamic linker
        let linker_paths = [
            self.path.join("lib64/ld-linux-x86-64.so.2"),
            self.path.join("lib/ld-linux-x86-64.so.2"),
        ];
        report.has_linker = linker_paths.iter().any(|p| p.exists());

        report.is_valid = report.missing_dirs.is_empty()
            && report.missing_tools.is_empty()
            && report.has_libc
            && report.has_linker;

        Ok(report)
    }

    /// Find a tool binary within the rootfs.
    fn find_in_rootfs(&self, tool: &str) -> Option<PathBuf> {
        let search_dirs = ["bin", "sbin", "usr/bin", "usr/sbin"];
        for dir in &search_dirs {
            let candidate = self.path.join(dir).join(tool);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    /// Remove the entire rootfs directory.
    pub fn remove(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path).map_err(|e| {
                SandboxError::CreationFailed(format!(
                    "failed to remove rootfs {}: {e}",
                    self.path.display()
                ))
            })?;
        }
        Ok(())
    }
}

/// Information about a single installed tool.
struct ToolInstallInfo {
    binary_path: PathBuf,
    libs: Vec<PathBuf>,
}

/// Report from rootfs population.
#[derive(Debug, Default)]
pub struct PopulateReport {
    /// Number of tools successfully installed.
    pub tools_installed: usize,
    /// Number of tools that could not be found/installed.
    pub tools_skipped: usize,
    /// Number of shared libraries copied.
    pub libs_copied: usize,
    /// Total files copied (binaries + libraries).
    pub files_copied: usize,
}

/// Report from rootfs verification.
#[derive(Debug, Default)]
pub struct VerifyReport {
    /// Whether the rootfs passes all checks.
    pub is_valid: bool,
    /// Number of directories verified.
    pub dirs_ok: usize,
    /// Number of tools verified.
    pub tools_ok: usize,
    /// Whether libc is present.
    pub has_libc: bool,
    /// Whether the dynamic linker is present.
    pub has_linker: bool,
    /// Missing directories.
    pub missing_dirs: Vec<String>,
    /// Missing tools.
    pub missing_tools: Vec<String>,
}

/// Find a binary on the host system PATH.
fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in path_var.split(':') {
        let candidate = PathBuf::from(dir).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Copy a single file, preserving permissions.
fn copy_file(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                SandboxError::CreationFailed(format!(
                    "failed to create parent directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }
    std::fs::copy(src, dest)
        .map_err(|e| {
            SandboxError::CreationFailed(format!(
                "failed to copy {} to {}: {e}",
                src.display(),
                dest.display()
            ))
        })
        .map(|_| ())
}

/// Resolve shared library dependencies of a binary using `ldd`.
fn resolve_libs(binary: &Path) -> Result<Vec<PathBuf>> {
    let output = std::process::Command::new("ldd")
        .arg(binary)
        .output()
        .map_err(|e| {
            SandboxError::CreationFailed(format!("failed to run ldd on {}: {e}", binary.display()))
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut libs = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        // ldd output format: "libname.so.1 => /lib/x86_64-linux-gnu/libname.so.1 (0x...)"
        // or: "/lib64/ld-linux-x86-64.so.2 (0x...)"
        if let Some(pos) = line.find("=> ") {
            let after = &line[pos + 3..];
            let path_str = after.split_whitespace().next().unwrap_or("");
            if !path_str.is_empty() {
                let path = PathBuf::from(path_str);
                if path.exists() {
                    libs.push(path);
                }
            }
        } else if line.starts_with('/') {
            // Direct absolute path (e.g., the dynamic linker itself)
            let path_str = line.split_whitespace().next().unwrap_or("");
            if !path_str.is_empty() {
                let path = PathBuf::from(path_str);
                if path.exists() {
                    libs.push(path);
                }
            }
        }
    }

    Ok(libs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_rootfs_create_directory_structure() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let rootfs_path = tmp.path().join("rootfs");
        let rootfs = MinimalRootfs::new(rootfs_path.clone());

        rootfs.create().expect("rootfs creation should succeed");

        // Verify essential directories
        assert!(rootfs_path.join("bin").is_dir());
        assert!(rootfs_path.join("lib").is_dir());
        assert!(rootfs_path.join("lib64").is_dir());
        assert!(rootfs_path.join("usr/bin").is_dir());
        assert!(rootfs_path.join("usr/lib").is_dir());
        assert!(rootfs_path.join("tmp").is_dir());
        assert!(rootfs_path.join("etc").is_dir());
        assert!(rootfs_path.join("srv").is_dir());
    }

    #[test]
    fn test_minimal_rootfs_create_idempotent() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let rootfs_path = tmp.path().join("rootfs");
        let rootfs = MinimalRootfs::new(rootfs_path);

        rootfs.create().expect("first creation should succeed");
        rootfs
            .create()
            .expect("second creation should also succeed");
    }

    #[test]
    fn test_minimal_rootfs_create_essential_files() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let rootfs_path = tmp.path().join("rootfs");
        let rootfs = MinimalRootfs::new(rootfs_path.clone());

        rootfs.create().expect("rootfs creation should succeed");
        rootfs
            .create_essential_files()
            .expect("essential files creation should succeed");

        // Verify essential files
        let passwd = std::fs::read_to_string(rootfs_path.join("etc/passwd")).unwrap();
        assert!(passwd.contains("root:x:0:0"));

        let group = std::fs::read_to_string(rootfs_path.join("etc/group")).unwrap();
        assert!(group.contains("root:x:0"));

        let hosts = std::fs::read_to_string(rootfs_path.join("etc/hosts")).unwrap();
        assert!(hosts.contains("127.0.0.1"));
    }

    #[test]
    fn test_verify_empty_rootfs() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let rootfs_path = tmp.path().join("rootfs");
        let rootfs = MinimalRootfs::new(rootfs_path.clone());

        rootfs.create().expect("rootfs creation should succeed");

        let report = rootfs.verify().expect("verify should succeed");
        assert!(!report.is_valid, "empty rootfs should not be valid");
        assert!(report.missing_tools.contains(&"sh".to_string()));
    }

    #[test]
    fn test_verify_with_fake_tools() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let rootfs_path = tmp.path().join("rootfs");

        // Create structure and fake tools
        std::fs::create_dir_all(rootfs_path.join("bin")).unwrap();
        std::fs::create_dir_all(rootfs_path.join("lib")).unwrap();
        std::fs::create_dir_all(rootfs_path.join("usr/bin")).unwrap();
        std::fs::create_dir_all(rootfs_path.join("tmp")).unwrap();
        std::fs::create_dir_all(rootfs_path.join("etc")).unwrap();
        std::fs::create_dir_all(rootfs_path.join("lib64")).unwrap();

        // Create fake essential binaries
        for tool in &["sh", "bash", "ls", "cat", "mkdir", "cp", "mv", "rm"] {
            std::fs::write(rootfs_path.join("bin").join(tool), "#!/bin/sh").unwrap();
        }

        // Create fake libc and linker
        std::fs::write(rootfs_path.join("lib/libc.so.6"), "fake").unwrap();
        std::fs::write(rootfs_path.join("lib64/ld-linux-x86-64.so.2"), "fake").unwrap();

        let rootfs = MinimalRootfs::new(rootfs_path);
        let report = rootfs.verify().expect("verify should succeed");
        assert!(report.is_valid, "rootfs with all tools should be valid");
        assert!(report.missing_tools.is_empty());
        assert!(report.has_libc);
        assert!(report.has_linker);
    }

    #[test]
    fn test_rootfs_remove() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let rootfs_path = tmp.path().join("rootfs");
        let rootfs = MinimalRootfs::new(rootfs_path.clone());

        rootfs.create().expect("rootfs creation should succeed");
        assert!(rootfs_path.exists());

        rootfs.remove().expect("rootfs removal should succeed");
        assert!(!rootfs_path.exists(), "rootfs should be removed");
    }

    #[test]
    fn test_which_finds_existing_binary() {
        // `ls` should exist on any Unix system
        let result = which("ls");
        assert!(result.is_some(), "ls should be found on PATH");
        let path = result.unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn test_which_returns_none_for_nonexistent() {
        let result = which("definitely_not_a_real_binary_xyz_123");
        assert!(result.is_none());
    }
}
