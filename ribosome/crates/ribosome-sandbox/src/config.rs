//! Sandbox configuration types for membrane build isolation.

use std::path::PathBuf;

/// Configuration for a build sandbox.
///
/// This config defines how `systemd-nspawn` will isolate the build environment.
/// By default, the sandbox uses the host's root filesystem as the container root
/// and only isolates via bind mounts for the build directories.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Root filesystem for the sandbox container.
    /// Defaults to "/" (host root) for nspawn.
    pub rootfs: PathBuf,

    /// Whether to isolate network access inside the sandbox.
    /// When true, `--private-network` is passed to nspawn.
    pub network_isolation: bool,

    /// Maximum memory the sandbox can use (e.g., "8G", "512M").
    /// Passed as `--property=MemoryMax=<value>` to nspawn.
    pub memory_limit: Option<String>,

    /// CPU quota for the sandbox (e.g., "50%", "2").
    /// Passed as `--property=CPUQuota=<value>` to nspawn.
    pub cpu_quota: Option<String>,

    /// Bind mounts to expose host directories into the sandbox.
    pub bind_mounts: Vec<BindMount>,

    /// Environment variables to inject into the sandbox.
    pub env_vars: Vec<(String, String)>,

    /// Working directory inside the sandbox where build scripts execute.
    pub working_dir: PathBuf,
}

impl SandboxConfig {
    /// Create a default sandbox config for building a package.
    ///
    /// Uses host root filesystem, with the build directories bind-mounted.
    pub fn new_for_build(build_base: PathBuf) -> Self {
        Self {
            rootfs: PathBuf::from("/"),
            network_isolation: false,
            memory_limit: None,
            cpu_quota: None,
            bind_mounts: vec![
                BindMount::new(build_base.join("src"), PathBuf::from("/srv/src"), false),
                BindMount::new(build_base.join("build"), PathBuf::from("/srv/build"), false),
                BindMount::new(build_base.join("pkg"), PathBuf::from("/srv/pkg"), true),
            ],
            env_vars: Vec::new(),
            working_dir: PathBuf::from("/srv/build"),
        }
    }

    /// Enable network isolation (offline build mode).
    pub fn with_network_isolation(mut self, enabled: bool) -> Self {
        self.network_isolation = enabled;
        self
    }

    /// Set memory limit for the sandbox.
    pub fn with_memory_limit(mut self, limit: impl Into<String>) -> Self {
        self.memory_limit = Some(limit.into());
        self
    }

    /// Set CPU quota for the sandbox.
    pub fn with_cpu_quota(mut self, quota: impl Into<String>) -> Self {
        self.cpu_quota = Some(quota.into());
        self
    }

    /// Add an environment variable to inject into the sandbox.
    ///
    /// # Panics
    ///
    /// Panics if `key` is empty or contains `=`.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        assert!(
            !key.is_empty(),
            "environment variable name must not be empty"
        );
        assert!(
            !key.contains('='),
            "environment variable name must not contain '=': {key}"
        );
        self.env_vars.push((key, value.into()));
        self
    }
}

/// A bind mount entry that maps a host directory into the sandbox.
#[derive(Debug, Clone)]
pub struct BindMount {
    /// Path on the host system.
    pub host_path: PathBuf,

    /// Path inside the sandbox container.
    pub sandbox_path: PathBuf,

    /// Whether the mount is writable from inside the sandbox.
    pub writable: bool,
}

impl BindMount {
    /// Create a new bind mount entry.
    pub fn new(host_path: PathBuf, sandbox_path: PathBuf, writable: bool) -> Self {
        Self {
            host_path,
            sandbox_path,
            writable,
        }
    }

    /// Convert to nspawn `--bind` argument format.
    /// Format: `--bind=<host_path>:<sandbox_path>` or `--bind=<host_path>:<sandbox_path>:rw`
    pub fn to_nspawn_arg(&self) -> String {
        let mut arg = format!(
            "--bind={}:{}",
            self.host_path.display(),
            self.sandbox_path.display()
        );
        if self.writable {
            arg.push_str(":rw");
        }
        arg
    }
}
