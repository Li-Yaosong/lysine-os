//! Bootstrap orchestrator for LFS system builds.
//!
//! Coordinates multi-phase LFS builds by applying the correct build profile
//! for each phase and tracking progress across packages.

use std::path::{Path, PathBuf};

use tracing::{info, warn};

use ribosome_parser::parse_mrna_file;
use ribosome_store::VacuoleStore;

use crate::context::{BuildConfig, BuildContext};
use crate::error::{CoreError, Result};
use crate::executor::BuildExecutor;
use crate::mrna_index::MrnaIndex;
use crate::profile::{self, BootstrapPhase};
use crate::progress::BuildProgress;
use crate::source;

/// Summary of a bootstrap run for a single phase.
#[derive(Debug)]
pub struct BootstrapPhaseReport {
    pub phase: BootstrapPhase,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub failures: Vec<String>,
}

/// Summary of a full bootstrap run across all phases.
#[derive(Debug, Default)]
pub struct BootstrapReport {
    pub phases: Vec<BootstrapPhaseReport>,
    pub total_succeeded: usize,
    pub total_failed: usize,
}

/// Run the bootstrap for a single phase.
///
/// Scans the nucleus directory for mRNA files, resolves the package list
/// for the given phase, and builds each package with the appropriate profile.
#[allow(clippy::too_many_arguments)]
pub fn bootstrap_phase(
    phase: BootstrapPhase,
    nucleus_dir: &Path,
    build_root: &Path,
    cache_dir: &Path,
    lock_file: Option<&Path>,
    continue_on_error: bool,
    clean: bool,
    progress: &dyn BuildProgress,
) -> Result<BootstrapPhaseReport> {
    // Ensure paths are absolute — relative paths break when bash changes working directory
    let build_root = if build_root.is_relative() {
        std::fs::canonicalize(build_root).unwrap_or_else(|_| {
            let cwd = std::env::current_dir().unwrap_or_default();
            cwd.join(build_root)
        })
    } else {
        build_root.to_path_buf()
    };
    let cache_dir = if cache_dir.is_relative() {
        let abs = std::env::current_dir().unwrap_or_default().join(cache_dir);
        std::fs::canonicalize(&abs).unwrap_or(abs)
    } else {
        cache_dir.to_path_buf()
    };

    // Derive the bootstrap base directory from cache_dir (cache_dir = base/cache)
    let bootstrap_base = cache_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| cache_dir.clone());

    // For the cross-toolchain and temp-tools phases, xgcc and other cross
    // tools expect to find binutils at /tools/x86_64-lysine-linux-gnu/bin/.
    // Try to create a symlink /tools -> <bootstrap_base>/tools so that the
    // hard-coded prefix paths work on the host system. This is a best-effort
    // operation — the PATH override in cross-gcc's mRNA makes the symlink
    // optional, so a failure (e.g. no root) is non-fatal.
    if phase == BootstrapPhase::CrossToolchain {
        if let Err(e) = ensure_tools_symlink(&bootstrap_base) {
            warn!("{} — builds will rely on PATH override in mRNA instead", e);
        }
    }

    let build_profile = profile::profile_for_phase(phase, &bootstrap_base);
    let package_specs = profile::packages_for_phase(phase);

    info!(
        phase = %build_profile.name,
        packages = package_specs.len(),
        prefix = %build_profile.prefix,
        "starting bootstrap phase"
    );

    // Scan nucleus for all available mRNA files
    let mut index = MrnaIndex::scan(nucleus_dir)?;
    if let Some(lock) = lock_file {
        index.load_version_lock(lock)?;
    }

    // Open CAS store for source extraction
    let vacuole_path = cache_dir.join("vacuole");
    let store =
        if vacuole_path.exists() {
            Some(VacuoleStore::open(&vacuole_path).map_err(|e| {
                CoreError::io(&vacuole_path, format!("failed to open vacuole: {e}"))
            })?)
        } else {
            None
        };

    let mut report = BootstrapPhaseReport {
        phase,
        total: package_specs.len(),
        succeeded: 0,
        failed: 0,
        skipped: 0,
        failures: Vec::new(),
    };

    for spec in package_specs.iter() {
        let entry = match index.resolve(spec) {
            Some(e) => e,
            None => {
                warn!(package = %spec.name, "mRNA not found, skipping");
                report.skipped += 1;
                continue;
            }
        };

        let mrna = match parse_mrna_file(&entry.path) {
            Ok(m) => m,
            Err(e) => {
                warn!(package = %spec.name, error = %e, "failed to parse mRNA");
                report.failed += 1;
                report.failures.push(format!("{}: parse error: {e}", spec));
                if !continue_on_error {
                    return Err(CoreError::BuildFailed {
                        package: spec.name.clone(),
                        reason: format!("parse error: {e}"),
                    });
                }
                continue;
            }
        };

        let pkg_index = report.succeeded + report.failed + report.skipped + 1;
        let pkg_label = format!("{}-{}", mrna.name, mrna.version);
        progress.package_started(pkg_index, package_specs.len(), &pkg_label);

        info!(
            package = %mrna.name,
            version = %mrna.version,
            phase = %build_profile.name,
            "building package"
        );

        // Create build config from profile
        let mut config = BuildConfig::new(&build_root);
        config.prefix = build_profile.prefix.clone();
        config.cflags = build_profile.cflags.clone();
        config.cxxflags = build_profile.cxxflags.clone();
        config.ldflags = build_profile.ldflags.clone();
        config.cache_dir = cache_dir.to_path_buf();
        config.clean = clean;

        // For cross-toolchain, temp-tools, kernel: install directly into bootstrap_base.
        // --prefix=/tools + DESTDIR=bootstrap_base → bootstrap_base/tools/bin/ld
        // For base-system: use default pkg/ DESTDIR, then install_to_dest merges into sysroot
        let use_direct_install = phase != BootstrapPhase::BaseSystem;
        if use_direct_install {
            config.destdir_override = Some(bootstrap_base.clone());
            config.skip_pack = true;
        }

        let ctx = BuildContext::new(mrna.clone(), config);

        // Skip package if all phases already completed (incremental build)
        if ctx.is_build_complete() {
            info!(
                package = %mrna.name,
                version = %mrna.version,
                "skipping completed package"
            );
            progress.package_finished(&pkg_label, true, std::time::Duration::ZERO);
            report.skipped += 1;
            continue;
        }

        // Extract source if CAS is available — skip if SRCDIR already populated
        // to preserve build artifacts from previous (incremental) runs.
        if let Some(ref store) = store {
            let src_dir = ctx.src_dir();
            let already_populated = src_dir.exists()
                && std::fs::read_dir(&src_dir).is_ok_and(|mut d| d.next().is_some());
            if !already_populated {
                source::extract_source(
                    &mrna,
                    store,
                    &src_dir,
                    Some(&|count, filename| progress.on_extract_file(count, filename)),
                )
                .map_err(|e| CoreError::BuildFailed {
                    package: mrna.name.clone(),
                    reason: format!("source extraction failed: {e}"),
                })?;

                progress.extract_done(0);
            } else {
                info!(package = %mrna.name, "SRCDIR already populated, skipping extraction");
            }
        }

        match BuildExecutor::build(&ctx, progress) {
            Ok(result) => {
                if result.is_ok() {
                    info!(
                        package = %result.package,
                        duration = ?result.total_duration,
                        "package built successfully"
                    );

                    // Base-system: merge pkg/ into dest_root (sysroot)
                    // Other phases: already installed directly via destdir_override
                    if !use_direct_install {
                        if let Err(e) = install_to_dest(&result.dest_dir, &build_profile.dest_root)
                        {
                            warn!(package = %mrna.name, error = %e, "install to dest_root failed");
                        }
                    }

                    progress.package_finished(&pkg_label, true, result.total_duration);
                    report.succeeded += 1;
                } else {
                    let phase_summary: Vec<String> = result
                        .phases
                        .iter()
                        .filter(|p| !p.success)
                        .map(|p| format!("{:?} failed", p.phase))
                        .collect();
                    let reason = if phase_summary.is_empty() {
                        "build did not complete".to_string()
                    } else {
                        format!("phases failed: {}", phase_summary.join(", "))
                    };
                    warn!(package = %mrna.name, reason = %reason, "build did not complete");
                    progress.package_finished(&pkg_label, false, result.total_duration);
                    report.failed += 1;
                    report.failures.push(format!("{}: {}", spec, reason));
                    if !continue_on_error {
                        return Err(CoreError::BuildFailed {
                            package: spec.name.clone(),
                            reason: "build did not complete".to_string(),
                        });
                    }
                }
            }
            Err(e) => {
                warn!(package = %mrna.name, error = %e, "build error");
                progress.package_finished(&pkg_label, false, std::time::Duration::ZERO);
                report.failed += 1;
                report.failures.push(format!("{}: {e:#}", spec));
                if !continue_on_error {
                    return Err(e);
                }
            }
        }
    }

    info!(
        phase = %build_profile.name,
        succeeded = report.succeeded,
        failed = report.failed,
        skipped = report.skipped,
        "phase complete"
    );

    Ok(report)
}

/// Run the full bootstrap (all phases).
pub fn bootstrap_all(
    nucleus_dir: &Path,
    build_root: &Path,
    cache_dir: &Path,
    lock_file: Option<&Path>,
    continue_on_error: bool,
    clean: bool,
    progress: &dyn BuildProgress,
) -> Result<BootstrapReport> {
    let phases = [
        BootstrapPhase::CrossToolchain,
        BootstrapPhase::TempTools,
        BootstrapPhase::BaseSystem,
        BootstrapPhase::Kernel,
    ];

    let mut report = BootstrapReport::default();

    for phase in &phases {
        let phase_report = bootstrap_phase(
            *phase,
            nucleus_dir,
            build_root,
            cache_dir,
            lock_file,
            continue_on_error,
            clean,
            progress,
        )?;

        report.total_succeeded += phase_report.succeeded;
        report.total_failed += phase_report.failed;

        let should_continue = phase_report.failed == 0 || continue_on_error;
        report.phases.push(phase_report);

        if !should_continue {
            warn!("stopping bootstrap due to failures");
            break;
        }

        // Between phases, set up chroot if needed
        if *phase == BootstrapPhase::TempTools {
            setup_chroot(build_root)?;
        }
    }

    Ok(report)
}

/// Copy built files from the build DESTDIR to the phase dest_root.
fn install_to_dest(src_dir: &Path, dest_dir: &Path) -> Result<()> {
    if !src_dir.exists() {
        return Ok(());
    }

    // Create dest parent
    if let Some(parent) = dest_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e.to_string()))?;
    }

    // Copy tree
    copy_dir_recursive(src_dir, dest_dir)?;

    Ok(())
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).map_err(|e| CoreError::io(dest, e.to_string()))?;

    for entry in std::fs::read_dir(src).map_err(|e| CoreError::io(src, e.to_string()))? {
        let entry = entry.map_err(|e| CoreError::io(src, e.to_string()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)
                .map_err(|e| CoreError::io(&dest_path, e.to_string()))?;
        }
    }

    Ok(())
}

/// Ensure `/tools` symlink exists, pointing to `<bootstrap_base>/tools`.
///
/// LFS cross-toolchain packages use `--prefix=/tools` at configure time.
/// GCC's xgcc then looks for the assembler and linker at
/// `-B/tools/x86_64-lysine-linux-gnu/bin/`. Since the actual tools are
/// installed into `<bootstrap_base>/tools/`, we create a top-level symlink
/// so the hard-coded prefix resolves correctly on the host system.
///
/// If the symlink already exists and points to the correct target, this is
/// a no-op. If it points elsewhere, the mismatch is logged as a warning.
fn ensure_tools_symlink(bootstrap_base: &Path) -> Result<()> {
    let tools_target = bootstrap_base.join("tools");
    let tools_link = PathBuf::from("/tools");

    // The target directory must exist (cross-binutils should have been built first)
    if !tools_target.exists() {
        std::fs::create_dir_all(&tools_target)
            .map_err(|e| CoreError::io(&tools_target, e.to_string()))?;
    }

    if tools_link.exists() {
        // Symlink or directory already exists — verify it points to the right place
        if let Ok(resolved) = tools_link.canonicalize() {
            let expected = tools_target
                .canonicalize()
                .unwrap_or_else(|_| tools_target.clone());
            if resolved == expected {
                info!(
                    "/tools symlink already points to {}",
                    tools_target.display()
                );
                return Ok(());
            }
            warn!(
                existing = %resolved.display(),
                expected = %expected.display(),
                "/tools exists but points to an unexpected location"
            );
        }
        return Ok(());
    }

    std::os::unix::fs::symlink(&tools_target, &tools_link).map_err(|e| {
        CoreError::io(
            &tools_link,
            format!(
                "failed to create /tools -> {} symlink (need root?): {e}",
                tools_target.display()
            ),
        )
    })?;

    info!("/tools -> {}", tools_target.display());

    Ok(())
}

/// Set up the chroot environment after temp-tools phase.
fn setup_chroot(_build_root: &Path) -> Result<()> {
    let base = PathBuf::from(profile::BOOTSTRAP_BASE);
    let sysroot = base.join("sysroot");
    let tools = base.join("tools");

    // Create sysroot directories
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
        "var/cache",
        "var/log",
        "var/tmp",
        "etc",
        "proc",
        "sys",
        "dev",
        "run",
        "tmp",
        "root",
    ];

    for dir in &dirs {
        let full = sysroot.join(dir);
        if !full.exists() {
            std::fs::create_dir_all(&full).map_err(|e| CoreError::io(&full, e.to_string()))?;
        }
    }

    // Symlink /tools into sysroot
    let tools_link = sysroot.join("tools");
    if !tools_link.exists() {
        std::os::unix::fs::symlink(&tools, &tools_link)
            .map_err(|e| CoreError::io(&tools_link, format!("symlink failed: {e}")))?;
    }

    // Essential symlinks: /bin -> usr/bin, /sbin -> usr/sbin, /lib -> usr/lib
    let symlinks = [("bin", "usr/bin"), ("sbin", "usr/sbin"), ("lib", "usr/lib")];

    for (link, target) in &symlinks {
        let link_path = sysroot.join(link);
        if !link_path.exists() {
            std::os::unix::fs::symlink(target, &link_path)
                .map_err(|e| CoreError::io(&link_path, format!("symlink failed: {e}")))?;
        }
    }

    info!("chroot environment prepared at {}", sysroot.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_to_dest_handles_missing_src() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("no-such-dir");
        let dest = tmp.path().join("dest");
        install_to_dest(&src, &dest).unwrap();
    }

    #[test]
    fn setup_chroot_creates_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("bootstrap");
        let tools = base.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        assert!(tools.exists());
    }
}
