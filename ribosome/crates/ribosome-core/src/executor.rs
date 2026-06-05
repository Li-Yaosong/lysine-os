use std::io::Write;
use std::process::Command;

use tracing::{info, warn};

use crate::context::{BuildContext, BuildPhase, BuildResult, PhaseResult};
use crate::error::{CoreError, Result};

/// Drives the four-phase build lifecycle from a parsed mRNA.
///
/// **SAFETY NOTICE (Sprint 1):**
/// Build scripts execute directly on the host via `bash -e -c` with **no sandboxing**.
/// The membrane isolation layer (Linux namespaces + cgroups + Btrfs subvolume) is
/// planned for Sprint 3. Until then, **only run `ribosome build` with trusted mRNA
/// files** on development machines — never in CI or shared environments.
///
/// Sprint 1 does NOT perform: source fetching, patch application, or network isolation.
pub struct BuildExecutor;

impl BuildExecutor {
    /// Run a full build for the given context.
    ///
    /// Creates the directory layout, then executes each non-empty build phase
    /// from the mRNA in order: prepare → compile → check → install.
    /// Every phase's stdout+stderr is appended to the transcript log.
    pub fn build(ctx: &BuildContext) -> Result<BuildResult> {
        let start = std::time::Instant::now();
        let package = ctx.mrna.name.clone();
        let version = ctx.mrna.version.clone();

        info!(package = %package, version = %version, "starting build");

        ctx.create_dirs()?;

        // Defensive: reject mRNA without build block (should have been caught by parser)
        if ctx.mrna.build.is_none() {
            return Err(CoreError::BuildFailed {
                package: package.clone(),
                reason: "mRNA must contain a build block (at minimum an install step)".to_string(),
            });
        }

        // Initialise transcript file
        let mut transcript = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(ctx.transcript_path())
            .map_err(|e| CoreError::io(ctx.transcript_path(), e.to_string()))?;

        writeln!(transcript, "=== ribosome build: {package}-{version} ===")
            .map_err(|e| CoreError::io(ctx.transcript_path(), e.to_string()))?;

        let build_script = ctx.mrna.build.as_ref();
        let phases: Vec<(BuildPhase, Option<&str>)> = vec![
            (
                BuildPhase::Prepare,
                build_script.and_then(|b| b.prepare.as_deref()),
            ),
            (
                BuildPhase::Compile,
                build_script.and_then(|b| b.compile.as_deref()),
            ),
            (
                BuildPhase::Check,
                build_script.and_then(|b| b.check.as_deref()),
            ),
            (
                BuildPhase::Install,
                build_script.and_then(|b| b.install.as_deref()),
            ),
        ];

        let mut phase_results = Vec::new();

        for (phase, script) in &phases {
            let script = match script {
                Some(s) if !s.trim().is_empty() => *s,
                _ => {
                    info!(phase = %phase, "skipped (no script)");
                    continue;
                }
            };

            let result = Self::run_phase(ctx, *phase, script, &mut transcript)?;
            if !result.success {
                warn!(phase = %phase, "build phase failed");
                let total = start.elapsed();
                return Ok(BuildResult {
                    package,
                    version,
                    success: false,
                    phases: phase_results,
                    dest_dir: ctx.dest_dir(),
                    total_duration: total,
                    protein: None,
                    pack_error: None,
                });
            }
            phase_results.push(result);
        }

        let total = start.elapsed();
        info!(
            package = %package,
            version = %version,
            elapsed = ?total,
            "build phases completed successfully"
        );

        // Auto-pack into .prot
        let (protein, pack_error) = match Self::pack_result(ctx, &total) {
            Ok(p) => (Some(p), None),
            Err(e) => {
                let msg = format!("{e}");
                warn!(error = %msg, "packing failed — build phases succeeded but .prot was not created");
                (None, Some(msg))
            }
        };

        // Packing failure means the overall build is not fully successful
        let success = pack_error.is_none();

        Ok(BuildResult {
            package,
            version,
            success,
            phases: phase_results,
            dest_dir: ctx.dest_dir(),
            total_duration: total,
            protein,
            pack_error,
        })
    }

    fn pack_result(
        ctx: &BuildContext,
        duration: &std::time::Duration,
    ) -> crate::Result<super::ProteinOutput> {
        use ribosome_package::{pack, PackageMeta};

        let mrna_yaml = serde_yaml::to_string(&ctx.mrna).map_err(|e| CoreError::BuildFailed {
            package: ctx.mrna.name.clone(),
            reason: format!("failed to serialize mRNA: {e}"),
        })?;

        let mut depends_build = Vec::new();
        let mut depends_runtime = Vec::new();
        if let Some(dep) = &ctx.mrna.depends {
            if let Some(b) = &dep.build {
                depends_build = b.clone();
            }
            if let Some(r) = &dep.runtime {
                depends_runtime = r.clone();
            }
        }

        let meta = PackageMeta {
            name: ctx.mrna.name.clone(),
            version: ctx.mrna.version.clone(),
            release: ctx.mrna.release,
            arch: ctx.config.arch.clone(),
            mrna_yaml,
            depends_build,
            depends_runtime,
            post_install: ctx.mrna.post_install.clone(),
            post_remove: ctx.mrna.post_remove.clone(),
            build_duration: *duration,
        };

        let pack_result = pack(&ctx.dest_dir(), &meta, &ctx.config.cache_dir).map_err(|e| {
            CoreError::BuildFailed {
                package: ctx.mrna.name.clone(),
                reason: format!("packing failed: {e}"),
            }
        })?;

        Ok(super::ProteinOutput {
            path: pack_result.path,
            sha256: pack_result.sha256,
            file_count: pack_result.file_count,
            size_bytes: pack_result.size_bytes,
        })
    }

    /// Execute a single build phase as a shell subprocess.
    fn run_phase(
        ctx: &BuildContext,
        phase: BuildPhase,
        script: &str,
        transcript: &mut std::fs::File,
    ) -> Result<PhaseResult> {
        let phase_start = std::time::Instant::now();
        info!(phase = %phase, "executing");

        writeln!(transcript, "\n--- phase: {} ({}) ---", phase, chrono_now())
            .map_err(|e| CoreError::io(ctx.transcript_path(), e.to_string()))?;

        let working_dir = match phase {
            BuildPhase::Prepare => ctx.src_dir(),
            _ => ctx.build_dir(),
        };
        // Ensure working directory exists
        std::fs::create_dir_all(&working_dir)
            .map_err(|e| CoreError::io(&working_dir, e.to_string()))?;

        let mut cmd = Command::new("bash");
        cmd.arg("-e").arg("-c").arg(script);
        cmd.current_dir(&working_dir);

        // Inject environment variables
        for (key, value) in ctx.env_vars() {
            cmd.env(key, value);
        }

        let output = cmd.output().map_err(|e| CoreError::CommandFailed {
            package: ctx.mrna.name.clone(),
            phase: phase.to_string(),
            message: format!("failed to spawn bash: {e}"),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Append to transcript
        if !stdout.is_empty() {
            transcript
                .write_all(stdout.as_bytes())
                .map_err(|e| CoreError::io(ctx.transcript_path(), e.to_string()))?;
        }
        if !stderr.is_empty() {
            transcript
                .write_all(stderr.as_bytes())
                .map_err(|e| CoreError::io(ctx.transcript_path(), e.to_string()))?;
        }

        let success = output.status.success();
        let log_output = format!("{stdout}{stderr}");

        if !success {
            warn!(
                phase = %phase,
                exit_code = ?output.status.code(),
                "phase failed"
            );
        }

        Ok(PhaseResult {
            phase,
            success,
            duration: phase_start.elapsed(),
            log_output,
        })
    }
}

fn chrono_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildConfig, BuildContext, BuildExecutor};
    use ribosome_parser::{parse_mrna, MrnaFile};

    fn minimal_mrna() -> MrnaFile {
        let yaml = r#"
api-version: 1
name: test-pkg
version: 1.0.0
release: 1
description: Test package
license: MIT
sources:
  - url: https://example.com/test-1.0.0.tar.xz
build:
  prepare: |
    echo "prepare"
  compile: |
    echo "compile"
  install: |
    echo "install" > "$DESTDIR/output.txt"
"#;
        parse_mrna(yaml).expect("valid mRNA")
    }

    #[test]
    fn build_context_dirs() {
        let mrna = minimal_mrna();
        let config = BuildConfig::new("/tmp/ribosome-test");
        let ctx = BuildContext::new(mrna, config);

        assert!(ctx.src_dir().ends_with("src"));
        assert!(ctx.build_dir().ends_with("build"));
        assert!(ctx.dest_dir().ends_with("pkg"));
        assert!(ctx.transcript_path().ends_with("transcript.log"));
    }

    #[test]
    fn env_vars_include_required_keys() {
        let mrna = minimal_mrna();
        let config = BuildConfig::new("/tmp/ribosome-test");
        let ctx = BuildContext::new(mrna, config);
        let vars = ctx.env_vars();
        let keys: Vec<&str> = vars.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"DESTDIR"));
        assert!(keys.contains(&"SRCDIR"));
        assert!(keys.contains(&"BUILDDIR"));
        assert!(keys.contains(&"NPROC"));
        assert!(keys.contains(&"ARCH"));
        assert!(keys.contains(&"PREFIX"));
    }

    #[test]
    fn phase_order_and_names() {
        assert_eq!(BuildPhase::Prepare.as_str(), "prepare");
        assert_eq!(BuildPhase::Compile.as_str(), "compile");
        assert_eq!(BuildPhase::Check.as_str(), "check");
        assert_eq!(BuildPhase::Install.as_str(), "install");
    }

    #[test]
    fn create_dirs_and_run_build() {
        let mrna = minimal_mrna();
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = BuildConfig::new(tmp.path());
        let ctx = BuildContext::new(mrna, config);

        ctx.create_dirs().expect("create dirs");
        assert!(ctx.src_dir().exists());
        assert!(ctx.build_dir().exists());
        assert!(ctx.dest_dir().exists());

        let result = BuildExecutor::build(&ctx).expect("build should not error");
        assert!(result.is_ok(), "build should succeed");
        assert_eq!(result.package, "test-pkg");
        assert_eq!(result.version, "1.0.0");
        assert!(result.phases.len() >= 3, "should have at least 3 phases");

        // Verify install phase wrote the file
        let output_file = ctx.dest_dir().join("output.txt");
        assert!(output_file.exists(), "install output file should exist");

        // Verify transcript was written
        let transcript = std::fs::read_to_string(ctx.transcript_path()).expect("read transcript");
        assert!(
            transcript.contains("ribosome build"),
            "transcript should have header"
        );

        // Verify .prot was created
        assert!(
            result.protein.is_some(),
            "protein should be packed after successful build"
        );
        let protein = result.protein.as_ref().unwrap();
        assert!(protein.path.exists(), ".prot file should exist");
        assert!(protein.sha256.starts_with("sha256:"));
        assert!(protein.file_count > 0, "should have packed at least 1 file");
    }

    #[test]
    fn failing_phase_returns_unsuccessful() {
        let yaml = r#"
api-version: 1
name: fail-pkg
version: 1.0.0
release: 1
description: Package that fails
license: MIT
sources:
  - url: https://example.com/test.tar.xz
build:
  compile: |
    exit 1
  install: |
    echo "should not reach"
"#;
        let mrna = parse_mrna(yaml).expect("valid mRNA");
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = BuildConfig::new(tmp.path());
        let ctx = BuildContext::new(mrna, config);

        let result = BuildExecutor::build(&ctx).expect("build should not error");
        assert!(!result.is_ok(), "build should report failure");
    }
}
