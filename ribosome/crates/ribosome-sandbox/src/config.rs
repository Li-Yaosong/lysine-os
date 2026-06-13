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

    /// Enable user namespace for unprivileged builds.
    /// When true, `--private-users` is passed to nspawn.
    pub user_namespace: bool,

    /// UID mapping for user namespace (e.g., "0:1000:1").
    /// Format: `"container_uid:host_uid:count"`.
    pub uid_map: Option<String>,

    /// GID mapping for user namespace (e.g., "0:1000:1").
    /// Format: `"container_gid:host_gid:count"`.
    pub gid_map: Option<String>,

    /// Capabilities to drop from the sandbox.
    /// Passed as `--drop-capability=<caps>` to nspawn.
    /// Example: `["CAP_SYS_ADMIN", "CAP_SYS_PTRACE"]`.
    pub drop_capabilities: Vec<String>,

    /// System call filter configuration.
    /// Passed as `--system-call-filter=<filter>` to nspawn.
    /// Use `~` prefix to prohibit syscalls (e.g., "~ptrace").
    /// Use `@group` syntax for syscall groups (e.g., "@clock").
    pub syscall_filter: Vec<String>,
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
            working_dir: PathBuf::from("/srv/src"),
            user_namespace: false,
            uid_map: None,
            gid_map: None,
            drop_capabilities: Vec::new(),
            syscall_filter: Vec::new(),
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

    /// Enable user namespace for unprivileged builds.
    ///
    /// When enabled, `--private-users` is passed to `systemd-nspawn`, allowing
    /// the sandbox to run without root privileges. The UID/GID inside the
    /// container are mapped so that the building user appears as root (UID 0).
    ///
    /// If `uid_map` / `gid_map` are not explicitly set, nspawn will use its
    /// default UID allocation (typically starting from 131072).
    pub fn with_user_namespace(mut self, enabled: bool) -> Self {
        self.user_namespace = enabled;
        self
    }

    /// Set explicit UID mapping for the user namespace.
    ///
    /// Format: `"container_uid:host_uid:count"` (e.g., `"0:1000:1"`).
    pub fn with_uid_map(mut self, map: impl Into<String>) -> Self {
        self.uid_map = Some(map.into());
        self
    }

    /// Set explicit GID mapping for the user namespace.
    ///
    /// Format: `"container_gid:host_gid:count"` (e.g., `"0:1000:1"`).
    pub fn with_gid_map(mut self, map: impl Into<String>) -> Self {
        self.gid_map = Some(map.into());
        self
    }

    /// Add a capability to drop from the sandbox.
    ///
    /// May be called multiple times. Each capability is passed to
    /// `--drop-capability=` on the nspawn command line.
    ///
    /// Example: `"CAP_SYS_PTRACE"`, `"CAP_SYS_ADMIN"`.
    pub fn with_drop_capability(mut self, cap: impl Into<String>) -> Self {
        self.drop_capabilities.push(cap.into());
        self
    }

    /// Add a system call filter rule.
    ///
    /// Each entry is passed to `--system-call-filter=` on the nspawn command
    /// line. Use `~` prefix to prohibit syscalls, or `@group` for syscall
    /// groups (e.g., `"@clock"`, `"~ptrace"`).
    ///
    /// May be called multiple times to combine positive and negative lists.
    pub fn with_syscall_filter(mut self, filter: impl Into<String>) -> Self {
        self.syscall_filter.push(filter.into());
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
