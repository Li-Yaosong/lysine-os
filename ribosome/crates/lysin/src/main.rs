use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "lysin")]
#[command(about = "LysineOS package manager")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Install a package
    Install { package: String },
    /// Remove a package
    Remove { package: String },
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
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Install { package } => {
            tracing::info!("Installing package: {package}");
        }
        Commands::Remove { package } => {
            tracing::info!("Removing package: {package}");
        }
        Commands::Update => {
            tracing::info!("Updating all packages");
        }
        Commands::Search { keyword } => {
            tracing::info!("Searching for: {keyword}");
        }
        Commands::Info { package } => {
            tracing::info!("Package info: {package}");
        }
        Commands::List => {
            tracing::info!("Listing installed packages");
        }
        Commands::Deps { package } => {
            tracing::info!("Dependencies for: {package}");
        }
        Commands::History => {
            tracing::info!("Operation history");
        }
        Commands::Rollback { snapshot } => {
            tracing::info!("Rolling back to snapshot: {snapshot}");
        }
        Commands::Autoremove => {
            tracing::info!("Removing orphaned dependencies");
        }
        Commands::Provenance { package } => {
            tracing::info!("Build provenance for: {package}");
        }
    }

    Ok(())
}
