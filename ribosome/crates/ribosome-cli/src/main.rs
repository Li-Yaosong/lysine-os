use anyhow::Result;
use clap::Parser;

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
        /// Package to build
        package: String,
    },
    /// Enter build sandbox for debugging
    Shell {
        /// Package sandbox to enter
        package: String,
    },
    /// Verify package integrity
    Check {
        /// Package to verify
        package: String,
    },
    /// Visualize dependency graph
    Graph {
        /// Directory to scan for mRNA files
        path: Option<String>,
    },
    /// Clean build cache
    Clean,
    /// Show package information
    Info {
        /// Package to inspect
        package: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { package } => {
            tracing::info!("Building package: {package}");
        }
        Commands::Shell { package } => {
            tracing::info!("Entering sandbox for: {package}");
        }
        Commands::Check { package } => {
            tracing::info!("Checking package: {package}");
        }
        Commands::Graph { path } => {
            let p = path.unwrap_or_else(|| ".".to_string());
            tracing::info!("Generating dependency graph for: {p}");
        }
        Commands::Clean => {
            tracing::info!("Cleaning build cache");
        }
        Commands::Info { package } => {
            tracing::info!("Package info: {package}");
        }
    }

    Ok(())
}
