//! Membrane build sandbox management for Ribosome.
//!
//! Provides isolated build environments using `systemd-nspawn` containers.
//! Each package build runs in its own sandbox with:
//! - Separate mount, PID, and (optionally) network namespaces
//! - cgroup resource limits (memory, CPU)
//! - Bind-mounted build directories for input/output
//!
//! # Example
//!
//! ```no_run
//! use std::path::PathBuf;
//! use ribosome_sandbox::{SandboxConfig, SandboxHandle};
//!
//! let build_base = PathBuf::from("/var/ribosome/build/gcc-14.2.0");
//! let config = SandboxConfig::new_for_build(build_base.clone())
//!     .with_network_isolation(true)
//!     .with_memory_limit("8G");
//!
//! let handle = SandboxHandle::new(build_base, config);
//! handle.create().expect("sandbox creation failed");
//!
//! let output = handle.run_phase("make -j$(nproc)").expect("phase failed");
//! if output.success {
//!     println!("Phase completed!");
//! }
//!
//! handle.destroy().expect("cleanup failed");
//! ```

pub mod btrfs;
pub mod config;
pub mod error;
pub mod rootfs;
pub mod sandbox;

pub use config::{BindMount, SandboxConfig};
pub use error::{Result, SandboxError};
pub use rootfs::{MinimalRootfs, PopulateReport, RootfsSpec, VerifyReport};
pub use sandbox::{SandboxHandle, SandboxPhaseOutput};
