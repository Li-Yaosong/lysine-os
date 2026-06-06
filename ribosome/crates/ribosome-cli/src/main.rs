use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Parser;
use ribosome_deps::DependencyGraph;
use ribosome_parser::{collect_validation_issues, parse_mrna_file, Severity, ValidationIssue};
use ribosome_sandbox::{SandboxConfig, SandboxHandle};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "ribosome")]
#[command(about = "LysineOS build engine")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Build a package from mRNA
    Build {
        /// Path to the .mRNA file to build
        package: PathBuf,
        /// Build root directory (default: ./build)
        #[arg(long, default_value = "./build")]
        build_root: PathBuf,
        /// Number of parallel jobs (default: auto-detect)
        #[arg(long)]
        jobs: Option<usize>,
        /// Run build phases inside a membrane sandbox (systemd-nspawn)
        #[arg(long)]
        sandbox: bool,
        /// Isolate network access inside the sandbox (implies --sandbox)
        #[arg(long)]
        no_network: bool,
        /// Memory limit for the sandbox (e.g., "8G", "512M")
        #[arg(long)]
        memory_limit: Option<String>,
        /// Enable user namespace for unprivileged builds
        #[arg(long)]
        user_namespace: bool,
        /// UID mapping for user namespace (e.g., "0:1000:1")
        #[arg(long)]
        uid_map: Option<String>,
        /// GID mapping for user namespace (e.g., "0:1000:1")
        #[arg(long)]
        gid_map: Option<String>,
        /// Comma-separated list of capabilities to drop (e.g., "CAP_SYS_PTRACE,CAP_SYS_ADMIN")
        #[arg(long)]
        drop_capabilities: Option<String>,
        /// System call filter rule (may be repeated). Use ~prefix to deny, @group for groups
        #[arg(long)]
        syscall_filter: Vec<String>,
        /// Custom root filesystem path for the sandbox (default: host root "/")
        #[arg(long)]
        rootfs: Option<PathBuf>,
    },
    /// Enter build sandbox for debugging
    Shell {
        /// Package name or path to build directory
        package: String,
        /// Build root directory (default: ./build)
        #[arg(long, default_value = "./build")]
        build_root: PathBuf,
    },
    /// Verify mRNA file(s) syntax and semantics
    Check {
        /// mRNA file or directory to validate
        path: PathBuf,
    },
    /// Visualize dependency graph as DOT
    Graph {
        /// Directory to scan for mRNA files
        path: Option<PathBuf>,
        /// Write DOT to file instead of stdout
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Clean build cache
    Clean,
    /// Show package information
    Info {
        /// Package to inspect
        package: String,
    },
    /// Repository management commands
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
}

#[derive(clap::Subcommand)]
enum RepoAction {
    /// Initialize a new empty repository
    Init {
        /// Path for the new repository
        path: PathBuf,
    },
    /// Publish a .prot package to a repository
    Publish {
        /// Path to the .prot file to publish
        package: PathBuf,
        /// Repository root path
        #[arg(long)]
        repo: PathBuf,
        /// Package category (core, devel, desktop, ai, extra)
        #[arg(long, default_value = "core")]
        category: String,
    },
    /// Rebuild the repository index from existing .prot files
    Reindex {
        /// Repository root path
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt::init();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build {
            package,
            build_root,
            jobs,
            sandbox,
            no_network,
            memory_limit,
            user_namespace,
            uid_map,
            gid_map,
            drop_capabilities,
            syscall_filter,
            rootfs,
        } => cmd_build(BuildArgs {
            mrna_path: &package,
            build_root: &build_root,
            jobs,
            sandbox,
            no_network,
            memory_limit: memory_limit.as_deref(),
            user_namespace,
            uid_map: uid_map.as_deref(),
            gid_map: gid_map.as_deref(),
            drop_capabilities: drop_capabilities.as_deref(),
            syscall_filter: syscall_filter.as_slice(),
            rootfs: rootfs.as_deref(),
        }),
        Commands::Shell {
            package,
            build_root,
        } => cmd_shell(&package, &build_root),
        Commands::Check { path } => cmd_check(&path),
        Commands::Graph { path, output } => cmd_graph(path.as_deref(), output.as_deref()),
        Commands::Clean => {
            tracing::info!("Cleaning build cache");
            Ok(())
        }
        Commands::Info { package } => {
            tracing::info!("Package info: {package}");
            bail!("info not implemented in Sprint 1");
        }
        Commands::Repo { action } => cmd_repo(action),
    }
}

fn cmd_shell(package: &str, build_root: &Path) -> Result<()> {
    let build_base = if Path::new(package).exists() {
        PathBuf::from(package)
    } else {
        build_root.join(package)
    };

    if !build_base.exists() {
        bail!("build directory does not exist: {}", build_base.display());
    }

    let src_dir = build_base.join("src");
    let build_dir = build_base.join("build");

    if !src_dir.exists() || !build_dir.exists() {
        bail!(
            "build layout incomplete: need src/ and build/ under {}",
            build_base.display()
        );
    }

    println!("Entering sandbox at: {}", build_base.display());

    // Build sandbox config using the library API
    let config = SandboxConfig::new_for_build(build_base.clone())
        .with_env("DESTDIR", "/srv/pkg")
        .with_env("SRCDIR", "/srv/src")
        .with_env("BUILDDIR", "/srv/build");

    let handle = SandboxHandle::new(build_base, config);

    let mut cmd = handle.build_interactive_command();
    cmd.arg("--");

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    cmd.arg(shell);

    let status = cmd.status().context("failed to execute systemd-nspawn")?;

    if !status.success() {
        bail!("sandbox exited with code: {}", status.code().unwrap_or(-1));
    }

    Ok(())
}

fn cmd_check(path: &Path) -> Result<()> {
    let files = collect_mrna_paths(path)?;
    if files.is_empty() {
        bail!("no .mRNA files found under {}", path.display());
    }

    let mut failed = 0usize;
    for file in &files {
        let label = file.file_name().unwrap().to_string_lossy();
        match parse_mrna_file(file) {
            Ok(mrna) => {
                for issue in collect_validation_issues(&mrna)
                    .into_iter()
                    .filter(|i| i.severity == Severity::Warning)
                {
                    print_issue("WARN", &label, &issue);
                }
                println!("[OK] {label}");
            }
            Err(e) => {
                failed += 1;
                match &e {
                    ribosome_parser::ParserError::Validation { issues } => {
                        for issue in issues.iter().filter(|i| i.severity == Severity::Error) {
                            print_issue("ERROR", &label, issue);
                        }
                        for issue in issues.iter().filter(|i| i.severity == Severity::Warning) {
                            print_issue("WARN", &label, issue);
                        }
                    }
                    _ => eprintln!("[ERROR] {label}: {e}"),
                }
            }
        }
    }

    if failed > 0 {
        bail!("{failed} mRNA file(s) failed validation");
    }
    Ok(())
}

fn print_issue(level: &str, label: &str, issue: &ValidationIssue) {
    println!("[{level}] {label}: {}: {}", issue.field, issue.message);
}

fn cmd_graph(path: Option<&Path>, output: Option<&Path>) -> Result<()> {
    let root = path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("nucleus/core"));
    let mut graph = DependencyGraph::new();
    let loaded = graph
        .load_mrna_directory(&root)
        .with_context(|| format!("loading mRNA from {}", root.display()))?;

    if loaded.is_empty() {
        bail!("no .mRNA files found under {}", root.display());
    }

    if graph.has_cycle() {
        let cycle = graph.cycle_packages();
        eprintln!("warning: cycle detected among: {}", cycle.join(", "));
    }

    let dot = graph.to_dot();
    if let Some(out_path) = output {
        std::fs::write(out_path, &dot)
            .with_context(|| format!("writing DOT to {}", out_path.display()))?;
        println!(
            "wrote dependency graph ({} packages) to {}",
            graph.package_count(),
            out_path.display()
        );
    } else {
        print!("{dot}");
    }
    Ok(())
}

fn collect_mrna_paths(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files = Vec::new();
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("mRNA") {
            files.push(p.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

/// Parsed build command arguments.
struct BuildArgs<'a> {
    mrna_path: &'a Path,
    build_root: &'a Path,
    jobs: Option<usize>,
    sandbox: bool,
    no_network: bool,
    memory_limit: Option<&'a str>,
    user_namespace: bool,
    uid_map: Option<&'a str>,
    gid_map: Option<&'a str>,
    drop_capabilities: Option<&'a str>,
    syscall_filter: &'a [String],
    rootfs: Option<&'a Path>,
}

fn cmd_build(args: BuildArgs<'_>) -> Result<()> {
    let mrna = parse_mrna_file(args.mrna_path)
        .with_context(|| format!("failed to parse {}", args.mrna_path.display()))?;

    let label = format!("{}-{}", mrna.name, mrna.version);
    tracing::info!("building {label}");

    let use_sandbox = args.sandbox || args.no_network || args.user_namespace;

    if use_sandbox {
        println!("[SANDBOX] Build will run inside membrane sandbox (systemd-nspawn)");
        if args.no_network {
            println!("[SANDBOX] Network isolation enabled");
        }
        if args.user_namespace {
            println!("[SANDBOX] User namespace enabled (unprivileged mode)");
        }
    } else {
        eprintln!("\x1b[33m[WARN] Building without membrane sandbox — scripts execute directly on host.\x1b[0m");
        eprintln!("\x1b[33m        Use --sandbox for isolated builds.\x1b[0m\n");
    }

    let mut config = ribosome_core::BuildConfig::new(args.build_root);
    if let Some(j) = args.jobs {
        config.jobs = j;
    }

    if use_sandbox {
        let base_dir = config.build_root.join(&label);
        let mut sandbox_config = SandboxConfig::new_for_build(base_dir);
        if args.no_network {
            sandbox_config = sandbox_config.with_network_isolation(true);
        }
        if let Some(mem) = args.memory_limit {
            sandbox_config = sandbox_config.with_memory_limit(mem);
        }
        if args.user_namespace {
            sandbox_config = sandbox_config.with_user_namespace(true);
            if let Some(uid) = args.uid_map {
                sandbox_config = sandbox_config.with_uid_map(uid);
            }
            if let Some(gid) = args.gid_map {
                sandbox_config = sandbox_config.with_gid_map(gid);
            }
        }
        if let Some(caps) = args.drop_capabilities {
            for cap in caps.split(',') {
                let cap = cap.trim();
                if !cap.is_empty() {
                    sandbox_config = sandbox_config.with_drop_capability(cap);
                }
            }
        }
        for filter in args.syscall_filter {
            if !filter.is_empty() {
                sandbox_config = sandbox_config.with_syscall_filter(filter);
            }
        }
        if let Some(rootfs_path) = args.rootfs {
            sandbox_config.rootfs = rootfs_path.to_path_buf();
        }
        config.sandbox_config = Some(sandbox_config);
    }

    let ctx = ribosome_core::BuildContext::new(mrna, config);
    let result = ribosome_core::BuildExecutor::build(&ctx)
        .with_context(|| format!("build failed for {label}"))?;

    for phase in &result.phases {
        let status = if phase.success { "OK" } else { "FAIL" };
        println!(
            "  [{status}] {} ({:.1}s)",
            phase.phase,
            phase.duration.as_secs_f64()
        );
    }

    if result.is_ok() {
        println!(
            "[OK] {} — {} phases, {:.1}s total",
            result.package,
            result.phases.len(),
            result.total_duration.as_secs_f64()
        );
        println!("  dest: {}", result.dest_dir.display());
        println!("  transcript: {}", ctx.transcript_path().display());
        if let Some(protein) = &result.protein {
            println!(
                "  protein: {} ({}, {} files, {})",
                protein.path.display(),
                format_size(protein.size_bytes),
                protein.file_count,
                &protein.sha256[..22]
            );
        }
        Ok(())
    } else if let Some(pack_err) = &result.pack_error {
        bail!(
            "[FAIL] {} — build phases succeeded but packing failed: {pack_err}",
            result.package
        );
    } else {
        bail!(
            "[FAIL] {} — build did not complete successfully",
            result.package
        );
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{} MiB", bytes / (1024 * 1024))
    }
}

fn cmd_repo(action: RepoAction) -> Result<()> {
    match action {
        RepoAction::Init { path } => {
            let repo = ribosome_repository::Repository::create(&path)?;
            println!("Initialized empty repository at {}", path.display());
            for cat in ribosome_repository::CATEGORIES {
                println!("  created: {}/", cat);
            }
            drop(repo);
            Ok(())
        }
        RepoAction::Publish {
            package,
            repo,
            category,
        } => {
            let repository = ribosome_repository::Repository::open(&repo)?;
            repository.publish(&package, &category)?;
            println!("Published {} to {}", package.display(), repo.display());
            Ok(())
        }
        RepoAction::Reindex { path } => {
            let repo = ribosome_repository::Repository::open(&path)?;
            let count = repo.rebuild_index()?;
            println!("Rebuilt index: {} packages in {}", count, path.display());
            Ok(())
        }
    }
}
