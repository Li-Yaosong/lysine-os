//! Sandbox handle for managing membrane build isolation via systemd-nspawn.

use std::path::PathBuf;
use std::process::Command;

use tracing::{debug, info, warn};

use crate::config::SandboxConfig;
use crate::error::{Result, SandboxError};

/// Output from a sandboxed phase execution.
#[derive(Debug)]
pub struct SandboxPhaseOutput {
    /// Whether the phase completed successfully.
    pub success: bool,
    /// Captured stdout from the sandboxed process.
    pub stdout: String,
    /// Captured stderr from the sandboxed process.
    pub stderr: String,
    /// Exit code of the sandboxed process, if available.
    pub exit_code: Option<i32>,
}

/// A handle to an active build sandbox.
///
/// Manages the lifecycle of a `systemd-nspawn` container used for
/// isolated package building. Each `SandboxHandle` corresponds to
/// one package build, with its own directory layout and bind mounts.
pub struct SandboxHandle {
    config: SandboxConfig,
    /// The base directory on the host containing src/, build/, pkg/.
    build_base: PathBuf,
}

impl SandboxHandle {
    /// Create a new sandbox handle for the given build directory.
    ///
    /// The `build_base` should be `<build_root>/<name>-<version>/`,
    /// containing `src/`, `build/`, and `pkg/` subdirectories.
    pub fn new(build_base: PathBuf, config: SandboxConfig) -> Self {
        Self { config, build_base }
    }

    /// Prepare the sandbox directory layout.
    ///
    /// Creates `src/`, `build/`, `pkg/` directories under `build_base`
    /// if they don't already exist.
    pub fn create(&self) -> Result<()> {
        info!(base = %self.build_base.display(), "preparing sandbox directories");

        // Ensure all bind mount source directories exist.
        // For the default config this creates src/, build/, pkg/ under build_base.
        for mount in &self.config.bind_mounts {
            if !mount.host_path.exists() {
                std::fs::create_dir_all(&mount.host_path).map_err(|e| {
                    SandboxError::CreationFailed(format!(
                        "failed to create bind mount source {}: {e}",
                        mount.host_path.display()
                    ))
                })?;
            }
        }

        Ok(())
    }

    /// Run a build phase script inside the sandbox.
    ///
    /// Executes the given shell script via `systemd-nspawn` with the
    /// configured isolation settings. stdout and stderr are captured.
    pub fn run_phase(&self, script: &str) -> Result<SandboxPhaseOutput> {
        info!(
            rootfs = %self.config.rootfs.display(),
            "executing phase in sandbox"
        );

        let mut cmd = self.build_nspawn_command();

        // Append the actual command to run inside the container
        cmd.arg("--");
        cmd.arg("/bin/bash");
        cmd.arg("-e");
        cmd.arg("-c");
        cmd.arg(script);

        debug!(command = ?cmd, "nspawn command");

        let output = cmd.output().map_err(|e| {
            SandboxError::ExecutionFailed(format!("failed to spawn systemd-nspawn: {e}"))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let success = output.status.success();

        if !success {
            warn!(
                exit_code = ?output.status.code(),
                "sandboxed phase failed"
            );
        }

        Ok(SandboxPhaseOutput {
            success,
            stdout,
            stderr,
            exit_code: output.status.code(),
        })
    }

    /// Clean up sandbox resources.
    ///
    /// Removes the temporary rootfs copy (if any), but preserves
    /// build artifacts in `src/`, `build/`, `pkg/`.
    pub fn destroy(&self) -> Result<()> {
        info!(base = %self.build_base.display(), "cleaning up sandbox");
        debug!(
            rootfs = %self.config.rootfs.display(),
            "no custom rootfs to clean up (host rootfs is shared)"
        );
        // TODO: When custom rootfs is supported, clean up the temporary rootfs copy here.
        // The build directories (src/build/pkg) are preserved for debugging and packaging.
        Ok(())
    }

    /// Build the `systemd-nspawn` command with all configured arguments.
    fn build_nspawn_command(&self) -> Command {
        let mut cmd = Command::new("systemd-nspawn");

        // Root filesystem
        cmd.arg(format!("--directory={}", self.config.rootfs.display()));

        // Quiet mode to reduce nspawn's own output noise
        cmd.arg("--quiet");

        // Working directory
        cmd.arg(format!("--chdir={}", self.config.working_dir.display()));

        // Bind mounts
        for mount in &self.config.bind_mounts {
            cmd.arg(mount.to_nspawn_arg());
        }

        // Network isolation
        if self.config.network_isolation {
            cmd.arg("--private-network");
        }

        // cgroup resource limits
        if let Some(ref mem) = self.config.memory_limit {
            cmd.arg(format!("--property=MemoryMax={mem}"));
        }
        if let Some(ref cpu) = self.config.cpu_quota {
            cmd.arg(format!("--property=CPUQuota={cpu}"));
        }

        // Environment variables
        for (key, value) in &self.config.env_vars {
            cmd.arg(format!("--setenv={key}={value}"));
        }

        cmd
    }

    /// Build an interactive nspawn command for `ribosome shell`.
    ///
    /// Unlike `build_nspawn_command`, this omits `--quiet` so the user can
    /// see container output, and returns the command without appending
    /// the shell invocation.
    pub fn build_interactive_command(&self) -> Command {
        let mut cmd = Command::new("systemd-nspawn");

        cmd.arg(format!("--directory={}", self.config.rootfs.display()));

        // Working directory
        cmd.arg(format!("--chdir={}", self.config.working_dir.display()));

        // Bind mounts
        for mount in &self.config.bind_mounts {
            cmd.arg(mount.to_nspawn_arg());
        }

        // Environment variables
        for (key, value) in &self.config.env_vars {
            cmd.arg(format!("--setenv={key}={value}"));
        }

        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BindMount;

    fn test_config() -> SandboxConfig {
        SandboxConfig::new_for_build(PathBuf::from("/tmp/test-build"))
    }

    #[test]
    fn test_sandbox_config_default_values() {
        let config = test_config();
        assert_eq!(config.rootfs, PathBuf::from("/"));
        assert!(!config.network_isolation);
        assert!(config.memory_limit.is_none());
        assert!(config.cpu_quota.is_none());
        assert_eq!(config.bind_mounts.len(), 3);
        assert_eq!(config.working_dir, PathBuf::from("/srv/build"));
    }

    #[test]
    fn test_bind_mount_nspawn_arg_readonly() {
        let mount = BindMount::new(PathBuf::from("/host/src"), PathBuf::from("/srv/src"), false);
        let arg = mount.to_nspawn_arg();
        assert_eq!(arg, "--bind=/host/src:/srv/src");
    }

    #[test]
    fn test_bind_mount_nspawn_arg_writable() {
        let mount = BindMount::new(PathBuf::from("/host/pkg"), PathBuf::from("/srv/pkg"), true);
        let arg = mount.to_nspawn_arg();
        assert_eq!(arg, "--bind=/host/pkg:/srv/pkg:rw");
    }

    #[test]
    fn test_nspawn_command_construction() {
        let config = test_config();
        let handle = SandboxHandle::new(PathBuf::from("/tmp/test-build"), config);
        let cmd = handle.build_nspawn_command();

        // Verify the command program
        assert_eq!(cmd.get_program(), "systemd-nspawn");

        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let args_joined = args.join(" ");

        // Must have directory flag
        assert!(args_joined.contains("--directory=/"));

        // Must have quiet flag
        assert!(args_joined.contains("--quiet"));

        // Must have chdir
        assert!(args_joined.contains("--chdir=/srv/build"));

        // Must have bind mounts for src, build, pkg
        assert!(args_joined.contains("--bind=/tmp/test-build/src:/srv/src"));
        assert!(args_joined.contains("--bind=/tmp/test-build/build:/srv/build"));
        assert!(args_joined.contains("--bind=/tmp/test-build/pkg:/srv/pkg:rw"));

        // Should NOT have network isolation by default
        assert!(!args_joined.contains("--private-network"));
    }

    #[test]
    fn test_nspawn_network_isolation_flag() {
        let config = test_config().with_network_isolation(true);
        let handle = SandboxHandle::new(PathBuf::from("/tmp/test-build"), config);
        let cmd = handle.build_nspawn_command();

        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let args_joined = args.join(" ");

        assert!(args_joined.contains("--private-network"));
    }

    #[test]
    fn test_nspawn_memory_limit() {
        let config = test_config().with_memory_limit("8G");
        let handle = SandboxHandle::new(PathBuf::from("/tmp/test-build"), config);
        let cmd = handle.build_nspawn_command();

        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let args_joined = args.join(" ");

        assert!(args_joined.contains("--property=MemoryMax=8G"));
    }

    #[test]
    fn test_nspawn_env_vars() {
        let config = test_config()
            .with_env("DESTDIR", "/srv/pkg")
            .with_env("SRCDIR", "/srv/src");
        let handle = SandboxHandle::new(PathBuf::from("/tmp/test-build"), config);
        let cmd = handle.build_nspawn_command();

        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let args_joined = args.join(" ");

        assert!(args_joined.contains("--setenv=DESTDIR=/srv/pkg"));
        assert!(args_joined.contains("--setenv=SRCDIR=/srv/src"));
    }

    #[test]
    fn test_sandbox_create_builds_directory_layout() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let build_base = tmp.path().join("test-pkg-1.0.0");
        let config = SandboxConfig::new_for_build(build_base.clone());
        let handle = SandboxHandle::new(build_base.clone(), config);

        handle.create().expect("sandbox create should succeed");

        assert!(build_base.join("src").exists(), "src/ should be created");
        assert!(
            build_base.join("src").is_dir(),
            "src/ should be a directory"
        );
        assert!(
            build_base.join("build").exists(),
            "build/ should be created"
        );
        assert!(
            build_base.join("build").is_dir(),
            "build/ should be a directory"
        );
        assert!(build_base.join("pkg").exists(), "pkg/ should be created");
        assert!(
            build_base.join("pkg").is_dir(),
            "pkg/ should be a directory"
        );
    }

    #[test]
    fn test_sandbox_create_idempotent() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let build_base = tmp.path().join("test-pkg-2.0.0");
        let config = SandboxConfig::new_for_build(build_base.clone());
        let handle = SandboxHandle::new(build_base.clone(), config);

        handle.create().expect("first create should succeed");
        handle.create().expect("second create should also succeed");
    }
}
