use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Parser;
use ribosome_deps::DependencyGraph;
use ribosome_parser::{collect_validation_issues, parse_mrna_file, Severity, ValidationIssue};
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
    },
    /// Enter build sandbox for debugging
    Shell {
        /// Package sandbox to enter
        package: String,
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
        Commands::Build { package, build_root, jobs } => {
            cmd_build(&package, &build_root, jobs)
        }
        Commands::Shell { package } => {
            tracing::info!("Entering sandbox for: {package}");
            bail!("shell not implemented in Sprint 1");
        }
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
    }
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

fn cmd_build(mrna_path: &Path, build_root: &Path, jobs: Option<usize>) -> Result<()> {
    let mrna = parse_mrna_file(mrna_path)
        .with_context(|| format!("failed to parse {}", mrna_path.display()))?;

    let label = format!("{}-{}", mrna.name, mrna.version);
    tracing::info!("building {label}");

    let mut config = ribosome_core::BuildConfig::new(build_root);
    if let Some(j) = jobs {
        config.jobs = j;
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
