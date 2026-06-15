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
    /// Override DESTDIR. When set, build scripts see this as DESTDIR instead
    /// of the default `<base_dir>/pkg`. Used by bootstrap to install directly
    /// into the phase's dest_root (e.g. `/var/ribosome/bootstrap/tools`).
    pub destdir_override: Option<PathBuf>,
    /// Skip .prot packaging after build phases complete.
    ///
    /// Used by bootstrap when `destdir_override` points to a shared directory
    /// (like `build/bootstrap/`) that contains files from multiple packages
    /// and the build tree — packing it all into .prot would be enormous.
    pub skip_pack: bool,
    /// Force rebuild even if phase markers exist.
    pub clean: bool,
    /// Extra environment variables injected into every build phase.
    ///
    /// Used by bootstrap to expose sysroot paths (e.g. PKG_CONFIG_PATH,
    /// PATH) so packages can find dependencies installed by earlier phases.
    pub extra_env: Vec<(String, String)>,
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
            destdir_override: None,
            skip_pack: false,
            clean: false,
            extra_env: Vec::new(),
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
/// ├── .ribosome-markers/  # phase completion markers
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
        self.config
            .destdir_override
            .clone()
            .unwrap_or_else(|| self.base_dir.join("pkg"))
    }

    pub fn transcript_path(&self) -> PathBuf {
        self.base_dir.join("transcript.log")
    }

    /// Directory holding phase completion marker files.
    fn markers_dir(&self) -> PathBuf {
        self.base_dir.join(".ribosome-markers")
    }

    /// Path to the marker file for a specific phase.
    pub fn phase_marker_path(&self, phase: BuildPhase) -> PathBuf {
        self.markers_dir().join(phase.as_str())
    }

    /// Check whether a phase has already completed successfully.
    pub fn is_phase_done(&self, phase: BuildPhase) -> bool {
        !self.config.clean && self.phase_marker_path(phase).exists()
    }

    /// Write a marker file indicating a phase completed successfully.
    pub fn mark_phase_done(&self, phase: BuildPhase) -> Result<()> {
        let dir = self.markers_dir();
        std::fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e.to_string()))?;
        let marker = self.phase_marker_path(phase);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        std::fs::write(&marker, format!("{timestamp}\n"))
            .map_err(|e| CoreError::io(&marker, e.to_string()))?;
        Ok(())
    }

    /// Check whether all defined build phases have completed.
    ///
    /// Returns `true` only when every phase that has a script in the mRNA
    /// also has a corresponding marker file.
    pub fn is_build_complete(&self) -> bool {
        if self.config.clean {
            return false;
        }
        let Some(ref build) = self.mrna.build else {
            return false;
        };
        let phases: Vec<(BuildPhase, Option<&str>)> = vec![
            (BuildPhase::Prepare, build.prepare.as_deref()),
            (BuildPhase::Compile, build.compile.as_deref()),
            (BuildPhase::Check, build.check.as_deref()),
            (BuildPhase::Install, build.install.as_deref()),
        ];
        phases
            .iter()
            .filter(|(_, script)| script.is_some_and(|s| !s.trim().is_empty()))
            .all(|(phase, _)| self.is_phase_done(*phase))
    }

    /// Remove all phase markers, forcing a full rebuild on the next run.
    pub fn clean_markers(&self) -> Result<()> {
        let dir = self.markers_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| CoreError::io(&dir, e.to_string()))?;
        }
        Ok(())
    }

    /// Create the directory layout for a build.
    pub fn create_dirs(&self) -> Result<()> {
        for dir in &[self.src_dir(), self.build_dir(), self.dest_dir()] {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e.to_string()))?;
        }
        Ok(())
    }

    /// Build the shell environment variables injected into every build phase.
    pub fn env_vars(&self) -> Vec<(String, String)> {
        let mut vars: Vec<(String, String)> = vec![
            (
                "DESTDIR".to_string(),
                self.dest_dir().to_string_lossy().into_owned(),
            ),
            (
                "SRCDIR".to_string(),
                self.src_dir().to_string_lossy().into_owned(),
            ),
            (
                "BUILDDIR".to_string(),
                self.build_dir().to_string_lossy().into_owned(),
            ),
            ("NPROC".to_string(), self.config.jobs.to_string()),
            ("ARCH".to_string(), self.config.arch.clone()),
            ("PREFIX".to_string(), self.config.prefix.clone()),
        ];
        if !self.config.cflags.is_empty() {
            vars.push(("CFLAGS".to_string(), self.config.cflags.clone()));
        }
        if !self.config.cxxflags.is_empty() {
            vars.push(("CXXFLAGS".to_string(), self.config.cxxflags.clone()));
        }
        if !self.config.ldflags.is_empty() {
            vars.push(("LDFLAGS".to_string(), self.config.ldflags.clone()));
        }
        // Extra environment variables (e.g. PKG_CONFIG_PATH, PATH overrides)
        for (k, v) in &self.config.extra_env {
            vars.push((k.clone(), v.clone()));
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
