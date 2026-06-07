//! Build profile templates for LFS bootstrap stages.
//!
//! Each profile defines the environment variables, prefix paths, and compiler
//! flags appropriate for a specific LFS build phase.

use std::path::{Path, PathBuf};

/// Build profile: defines the build environment for a specific LFS phase.
#[derive(Debug, Clone)]
pub struct BuildProfile {
    /// Human-readable name.
    pub name: String,
    /// Installation prefix (e.g. /tools or /usr).
    pub prefix: String,
    /// Installation root (DESTDIR base, e.g. /var/ribosome/bootstrap/tools or sysroot).
    pub dest_root: PathBuf,
    /// Extra CFLAGS.
    pub cflags: String,
    /// Extra CXXFLAGS.
    pub cxxflags: String,
    /// Extra LDFLAGS.
    pub ldflags: String,
    /// Extra environment variables.
    pub extra_env: Vec<(String, String)>,
    /// Whether to use the cross-toolchain host triplet.
    pub cross_target: Option<String>,
}

/// A package reference with an optional version pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    /// Package name (must match mRNA `name` field).
    pub name: String,
    /// Optional version constraint (e.g. "7.0" to select a specific mRNA).
    pub version: Option<String>,
}

impl PackageSpec {
    /// Create a spec with name only (picks the latest available version).
    pub fn name_only(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: None,
        }
    }

    /// Create a spec with a pinned version.
    pub fn pinned(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: Some(version.to_string()),
        }
    }

    /// Check if a given mRNA version matches this spec.
    pub fn matches_version(&self, mrna_version: &str) -> bool {
        match &self.version {
            Some(v) => mrna_version == v,
            None => true,
        }
    }
}

impl std::fmt::Display for PackageSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.version {
            Some(v) => write!(f, "{}@{}", self.name, v),
            None => write!(f, "{}", self.name),
        }
    }
}

/// Available LFS bootstrap phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapPhase {
    /// Phase B: Cross-toolchain (LFS Chapter 5).
    CrossToolchain,
    /// Phase C: Temporary tools (LFS Chapter 6).
    TempTools,
    /// Phase D: Base system in chroot (LFS Chapter 7-8).
    BaseSystem,
    /// Phase D continuation: Kernel compilation.
    Kernel,
}

impl std::str::FromStr for BootstrapPhase {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "cross-toolchain" => Ok(Self::CrossToolchain),
            "temp-tools" => Ok(Self::TempTools),
            "base-system" => Ok(Self::BaseSystem),
            "kernel" => Ok(Self::Kernel),
            _ => Err(format!("unknown bootstrap phase: {s}")),
        }
    }
}

impl std::fmt::Display for BootstrapPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CrossToolchain => write!(f, "cross-toolchain"),
            Self::TempTools => write!(f, "temp-tools"),
            Self::BaseSystem => write!(f, "base-system"),
            Self::Kernel => write!(f, "kernel"),
        }
    }
}

/// The LFS target triplet for cross-compilation.
pub const LFS_TARGET: &str = "x86_64-lysine-linux-gnu";

/// Base directory for bootstrap artifacts.
pub const BOOTSTRAP_BASE: &str = "/var/ribosome/bootstrap";

/// Create a build profile for the given phase.
///
/// `base` is the bootstrap root directory (e.g. `/var/ribosome/bootstrap`
/// or a user-supplied local path). `dest_root` is derived from it.
pub fn profile_for_phase(phase: BootstrapPhase, base: &Path) -> BuildProfile {
    match phase {
        BootstrapPhase::CrossToolchain => BuildProfile {
            name: "cross-toolchain".to_string(),
            prefix: "/tools".to_string(),
            dest_root: base.join("tools"),
            cflags: String::new(),
            cxxflags: String::new(),
            ldflags: String::new(),
            extra_env: vec![
                ("LC_ALL".to_string(), "POSIX".to_string()),
                ("LFS_TGT".to_string(), LFS_TARGET.to_string()),
            ],
            cross_target: Some(LFS_TARGET.to_string()),
        },
        BootstrapPhase::TempTools => BuildProfile {
            name: "temp-tools".to_string(),
            prefix: "/tools".to_string(),
            dest_root: base.join("tools"),
            cflags: String::new(),
            cxxflags: String::new(),
            ldflags: String::new(),
            extra_env: vec![
                ("LC_ALL".to_string(), "POSIX".to_string()),
                ("LFS_TGT".to_string(), LFS_TARGET.to_string()),
            ],
            cross_target: None,
        },
        BootstrapPhase::BaseSystem => BuildProfile {
            name: "base-system".to_string(),
            prefix: "/usr".to_string(),
            dest_root: base.join("sysroot"),
            cflags: "-O2 -pipe -march=x86-64-v3 -mtune=generic -fstack-protector-strong"
                .to_string(),
            cxxflags: "-O2 -pipe -march=x86-64-v3 -mtune=generic -fstack-protector-strong"
                .to_string(),
            ldflags: "-Wl,-O1,--sort-common,--as-needed,-z,relro,-z,now".to_string(),
            extra_env: vec![("LC_ALL".to_string(), "POSIX".to_string())],
            cross_target: None,
        },
        BootstrapPhase::Kernel => BuildProfile {
            name: "kernel".to_string(),
            prefix: "/usr".to_string(),
            dest_root: base.join("sysroot"),
            cflags: String::new(),
            cxxflags: String::new(),
            ldflags: String::new(),
            extra_env: vec![("LC_ALL".to_string(), "POSIX".to_string())],
            cross_target: None,
        },
    }
}

/// Package lists for each phase, in build order.
/// These follow the LFS 13.0 (systemd) chapter order.
///
/// Use `PackageSpec::name_only` for packages where any available version is fine,
/// and `PackageSpec::pinned` to select a specific version.
pub fn packages_for_phase(phase: BootstrapPhase) -> Vec<PackageSpec> {
    match phase {
        BootstrapPhase::CrossToolchain => vec![
            PackageSpec::name_only("cross-binutils"),
            PackageSpec::name_only("cross-gcc"),
            PackageSpec::name_only("linux-api-headers"),
            PackageSpec::name_only("cross-glibc"),
            PackageSpec::name_only("cross-libstdcxx"),
        ],
        BootstrapPhase::TempTools => vec![
            PackageSpec::name_only("m4"),
            PackageSpec::name_only("ncurses"),
            PackageSpec::name_only("bash"),
            PackageSpec::name_only("coreutils"),
            PackageSpec::name_only("diffutils"),
            PackageSpec::name_only("file"),
            PackageSpec::name_only("findutils"),
            PackageSpec::name_only("gawk"),
            PackageSpec::name_only("grep"),
            PackageSpec::name_only("gzip"),
            PackageSpec::name_only("make"),
            PackageSpec::name_only("patch"),
            PackageSpec::name_only("sed"),
            PackageSpec::name_only("tar"),
            PackageSpec::name_only("xz"),
            PackageSpec::name_only("binutils"),
            PackageSpec::name_only("gcc"),
        ],
        BootstrapPhase::BaseSystem => vec![
            // Chapter 7-8: Base system (key packages in LFS order)
            PackageSpec::name_only("linux-api-headers"),
            PackageSpec::name_only("glibc"),
            PackageSpec::name_only("zlib"),
            PackageSpec::name_only("bzip2"),
            PackageSpec::name_only("xz"),
            PackageSpec::name_only("zstd"),
            PackageSpec::name_only("file"),
            PackageSpec::name_only("readline"),
            PackageSpec::name_only("m4"),
            PackageSpec::name_only("bc"),
            PackageSpec::name_only("flex"),
            PackageSpec::name_only("bison"),
            PackageSpec::name_only("coreutils"),
            PackageSpec::name_only("diffutils"),
            PackageSpec::name_only("gawk"),
            PackageSpec::name_only("findutils"),
            PackageSpec::name_only("grep"),
            PackageSpec::name_only("bash"),
            PackageSpec::name_only("libtool"),
            PackageSpec::name_only("gdbm"),
            PackageSpec::name_only("gperf"),
            PackageSpec::name_only("expat"),
            PackageSpec::name_only("inetutils"),
            PackageSpec::name_only("less"),
            PackageSpec::name_only("perl"),
            PackageSpec::name_only("python3"),
            PackageSpec::name_only("sed"),
            PackageSpec::name_only("shadow"),
            PackageSpec::name_only("pkgconf"),
            PackageSpec::name_only("ncurses"),
            PackageSpec::name_only("attr"),
            PackageSpec::name_only("acl"),
            PackageSpec::name_only("libcap"),
            PackageSpec::name_only("psmisc"),
            PackageSpec::name_only("openssl"),
            PackageSpec::name_only("kmod"),
            PackageSpec::name_only("e2fsprogs"),
            PackageSpec::name_only("procps-ng"),
            PackageSpec::name_only("util-linux"),
            PackageSpec::name_only("systemd"),
            PackageSpec::name_only("dbus"),
            PackageSpec::name_only("groff"),
            PackageSpec::name_only("texinfo"),
            PackageSpec::name_only("sudo"),
            PackageSpec::name_only("iana-etc"),
            PackageSpec::name_only("kbd"),
            PackageSpec::name_only("grub"),
        ],
        BootstrapPhase::Kernel => vec![PackageSpec::name_only("linux-kernel")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_toolchain_profile_has_tools_prefix() {
        let profile = profile_for_phase(BootstrapPhase::CrossToolchain, Path::new("/tmp/test"));
        assert_eq!(profile.prefix, "/tools");
        assert!(profile.cross_target.is_some());
    }

    #[test]
    fn base_system_profile_has_usr_prefix() {
        let profile = profile_for_phase(BootstrapPhase::BaseSystem, Path::new("/tmp/test"));
        assert_eq!(profile.prefix, "/usr");
        assert!(!profile.cflags.is_empty());
    }

    #[test]
    fn packages_for_cross_toolchain() {
        let pkgs = packages_for_phase(BootstrapPhase::CrossToolchain);
        assert!(pkgs.iter().any(|p| p.name == "cross-binutils"));
        assert!(pkgs.iter().any(|p| p.name == "cross-glibc"));
        assert!(pkgs.iter().any(|p| p.name == "cross-libstdcxx"));
        assert!(!pkgs.is_empty());
    }

    #[test]
    fn packages_for_base_system_includes_systemd() {
        let pkgs = packages_for_phase(BootstrapPhase::BaseSystem);
        assert!(pkgs.iter().any(|p| p.name == "systemd"));
        assert!(pkgs.iter().any(|p| p.name == "bash"));
    }

    #[test]
    fn phase_display() {
        assert_eq!(
            format!("{}", BootstrapPhase::CrossToolchain),
            "cross-toolchain"
        );
        assert_eq!(format!("{}", BootstrapPhase::BaseSystem), "base-system");
    }

    #[test]
    fn package_spec_name_only_has_no_version() {
        let spec = PackageSpec::name_only("linux-kernel");
        assert_eq!(spec.name, "linux-kernel");
        assert!(spec.version.is_none());
        assert!(spec.matches_version("7.0"));
        assert!(spec.matches_version("6.18.0"));
    }

    #[test]
    fn package_spec_pinned_matches_exact_version() {
        let spec = PackageSpec::pinned("linux-kernel", "7.0");
        assert_eq!(spec.name, "linux-kernel");
        assert_eq!(spec.version.as_deref(), Some("7.0"));
        assert!(spec.matches_version("7.0"));
        assert!(!spec.matches_version("6.18.0"));
    }

    #[test]
    fn package_spec_display() {
        assert_eq!(format!("{}", PackageSpec::name_only("bash")), "bash",);
        assert_eq!(
            format!("{}", PackageSpec::pinned("linux-kernel", "7.0")),
            "linux-kernel@7.0",
        );
    }
}
