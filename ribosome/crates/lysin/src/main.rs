use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use lysin::config;
use lysin::ops;

#[derive(Parser)]
#[command(name = "lysin")]
#[command(about = "LysineOS package manager")]
#[command(version)]
struct Cli {
    /// Installation root directory (default: /)
    #[arg(long, global = true, default_value = "/")]
    root: PathBuf,

    /// Repository path (can be specified multiple times)
    #[arg(long = "repo", global = true)]
    repositories: Vec<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Install a package
    Install { package: String },
    /// Remove a package
    Remove {
        package: String,
        /// Force removal even if other packages depend on it
        #[arg(long)]
        force: bool,
    },
    /// Update all packages
    Update,
    /// Search for a package
    Search { keyword: String },
    /// Show package information
    Info { package: String },
    /// List installed packages
    List,
    /// Show dependency tree
    Deps { package: String },
    /// Show operation history
    History,
    /// Rollback to a snapshot
    Rollback { snapshot: String },
    /// Remove orphaned dependencies
    Autoremove,
    /// Show package build provenance
    Provenance { package: String },
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt::init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    let mut lysin_config = config::LysinConfig::load_or_default(&cli.root);
    // Merge CLI repo flags into config.
    lysin_config.repositories = cli.repositories;

    match cli.command {
        Commands::Install { package } => {
            ops::install::install(&package, &lysin_config).await?;
        }
        Commands::Remove { package, force } => {
            ops::remove::remove(&package, &lysin_config, force).await?;
        }
        Commands::Update => {
            ops::update::update(&lysin_config).await?;
        }
        Commands::Search { keyword } => {
            ops::search::search(&keyword, &lysin_config).await?;
        }
        Commands::Info { package } => {
            ops::info::info(&package, &lysin_config).await?;
        }
        Commands::List => {
            ops::list::list(&lysin_config).await?;
        }
        Commands::Deps { package } => {
            ops::deps::deps(&package, &lysin_config).await?;
        }
        Commands::History => {
            tracing::info!("Operation history");
            println!("History not implemented yet (Sprint 3)");
        }
        Commands::Rollback { snapshot } => {
            tracing::info!("Rolling back to snapshot: {snapshot}");
            println!("Rollback not implemented yet (Sprint 3)");
        }
        Commands::Autoremove => {
            tracing::info!("Removing orphaned dependencies");
            println!("Autoremove not implemented yet (Sprint 3)");
        }
        Commands::Provenance { package } => {
            tracing::info!("Build provenance for: {package}");
            println!("Provenance not implemented yet (Sprint 3)");
        }
    }

    Ok(())
}
