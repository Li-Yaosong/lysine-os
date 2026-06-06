use std::path::PathBuf;

use ribosome_parser::MrnaFile;
use ribosome_sandbox::SandboxConfig;

use crate::error::{CoreError, Result};

/// Build phase names in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPhase {
    Prepare,
    Compile,
    Check,
    Install,
}

impl BuildPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::Compile => "compile",
            Self::Check => "check",
            Self::Install => "install",
        }
    }
}

impl std::fmt::Display for BuildPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Immutable configuration for a single package build.
#[derive(Debug, Clone)]
pub struct BuildConfig {
    /// Root directory for all build artifacts.
    pub build_root: PathBuf,
    /// Directory for .prot package output (vacuole cache).
    pub cache_dir: PathBuf,
    /// Number of parallel jobs (e.g. make -j).
    pub jobs: usize,
    /// Target architecture string.
    pub arch: String,
    /// Installation prefix.
    pub prefix: String,
    /// Extra CFLAGS.
    pub cflags: String,
    /// Extra CXXFLAGS.
    pub cxxflags: String,
    /// Extra LDFLAGS.
    pub ldflags: String,
    /// Sandbox configuration. When set, build phases run inside a membrane sandbox.
    pub sandbox_config: Option<SandboxConfig>,
}

impl BuildConfig {
    pub fn new(build_root: impl Into<PathBuf>) -> Self {
        let root = build_root.into();
        let cache_dir = root.join("cache");
        Self {
            build_root: root,
            cache_dir,
            jobs: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            arch: "x86_64".to_string(),
            prefix: "/usr".to_string(),
            cflags: String::new(),
            cxxflags: String::new(),
            ldflags: String::new(),
            sandbox_config: None,
        }
    }
}

/// Resolved directory layout for a single package build.
///
/// Layout:
/// ```text
/// <build_root>/<name>-<version>/
/// ├── src/          # SRCDIR  - extracted source tarball
/// ├── build/        # BUILDDIR - out-of-tree build directory
/// ├── pkg/          # DESTDIR - staged installation root
/// └── transcript.log
/// ```
pub struct BuildContext {
    pub mrna: MrnaFile,
    pub config: BuildConfig,
    /// Base directory: `<build_root>/<name>-<version>`
    pub base_dir: PathBuf,
}

impl BuildContext {
    pub fn new(mrna: MrnaFile, config: BuildConfig) -> Self {
        let base_dir = config
            .build_root
            .join(format!("{}-{}", mrna.name, mrna.version));
        Self {
            mrna,
            config,
            base_dir,
        }
    }

    pub fn src_dir(&self) -> PathBuf {
        self.base_dir.join("src")
    }

    pub fn build_dir(&self) -> PathBuf {
        self.base_dir.join("build")
    }

    pub fn dest_dir(&self) -> PathBuf {
        self.base_dir.join("pkg")
    }

    pub fn transcript_path(&self) -> PathBuf {
        self.base_dir.join("transcript.log")
    }

    /// Create the directory layout for a build.
    pub fn create_dirs(&self) -> Result<()> {
        for dir in &[self.src_dir(), self.build_dir(), self.dest_dir()] {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e.to_string()))?;
        }
        Ok(())
    }

    /// Build the shell environment variables injected into every build phase.
    pub fn env_vars(&self) -> Vec<(&'static str, String)> {
        let mut vars = vec![
            ("DESTDIR", self.dest_dir().to_string_lossy().into_owned()),
            ("SRCDIR", self.src_dir().to_string_lossy().into_owned()),
            ("BUILDDIR", self.build_dir().to_string_lossy().into_owned()),
            ("NPROC", self.config.jobs.to_string()),
            ("ARCH", self.config.arch.clone()),
            ("PREFIX", self.config.prefix.clone()),
        ];
        if !self.config.cflags.is_empty() {
            vars.push(("CFLAGS", self.config.cflags.clone()));
        }
        if !self.config.cxxflags.is_empty() {
            vars.push(("CXXFLAGS", self.config.cxxflags.clone()));
        }
        if !self.config.ldflags.is_empty() {
            vars.push(("LDFLAGS", self.config.ldflags.clone()));
        }
        vars
    }
}

/// Outcome of a completed build phase.
#[derive(Debug)]
pub struct PhaseResult {
    pub phase: BuildPhase,
    pub success: bool,
    pub duration: std::time::Duration,
    pub log_output: String,
}

/// Final result of a package build.
#[derive(Debug)]
pub struct BuildResult {
    pub package: String,
    pub version: String,
    pub success: bool,
    pub phases: Vec<PhaseResult>,
    pub dest_dir: PathBuf,
    pub total_duration: std::time::Duration,
    /// .prot package output, if build succeeded and pack succeeded.
    pub protein: Option<ProteinOutput>,
    /// Error message when the build phases succeeded but packing failed.
    /// When present, `success` is `false` and `protein` is `None`.
    pub pack_error: Option<String>,
}

/// Information about the produced .prot package.
#[derive(Debug)]
pub struct ProteinOutput {
    pub path: PathBuf,
    pub sha256: String,
    pub file_count: usize,
    pub size_bytes: u64,
}

impl BuildResult {
    pub fn is_ok(&self) -> bool {
        self.success
    }
}
