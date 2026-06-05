use anyhow::{bail, Context, Result};
use ribosome_package::unpack;
use ribosome_repository::Repository;
use tracing::{info, warn};

use crate::config::LysinConfig;
use crate::db::{InstalledPackage, LocalDb};

/// Install a package (and its dependencies) from the configured repositories.
pub async fn install(name: &str, config: &LysinConfig) -> Result<()> {
    config.ensure_dirs()?;

    let mut db = LocalDb::new(&config.db_path);
    db.load()?;

    if db.is_installed(name) {
        info!(package = name, "already installed");
        println!("{} is already installed", name);
        return Ok(());
    }

    // Find the package in the configured repositories.
    let (entry, repo) = find_package_in_repos(name, config)?;

    info!(package = name, version = %entry.version, "installing");
    println!("Installing {}-{}...", entry.name, entry.version);

    // Install dependencies first.
    for dep in &entry.depends.runtime {
        let dep_name = dep.split_whitespace().next().unwrap_or(dep);
        if !db.is_installed(dep_name) {
            info!(dependency = dep_name, "installing dependency");
            println!("  Installing dependency: {}", dep_name);
            install_single(dep_name, config, &mut db, &repo).await?;
        }
    }

    // Install the package itself.
    install_single(name, config, &mut db, &repo).await?;

    println!("Done.");
    Ok(())
}

/// Install a single package (no dependency resolution).
async fn install_single(
    name: &str,
    config: &LysinConfig,
    db: &mut LocalDb,
    repo: &Repository,
) -> Result<()> {
    let entry = repo
        .load_index()?
        .find(name)
        .cloned()
        .context(format!("package '{name}' not found in repository index"))?;

    let prot_path = repo.root.join(&entry.filename);
    if !prot_path.exists() {
        bail!(
            "package file not found: {} — run 'ribosome repo reindex' to rebuild the index",
            prot_path.display()
        );
    }

    // Verify hash.
    let actual_hash = hash_file(&prot_path)?;
    if actual_hash != entry.sha256 {
        bail!(
            "hash mismatch for {}: expected {}, got {}",
            prot_path.display(),
            entry.sha256,
            actual_hash
        );
    }

    // Extract to install root.
    let extracted = unpack(&prot_path, &config.root)
        .context(format!("unpacking {}", prot_path.display()))?;

    let file_list: Vec<String> = extracted
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    // Record in database.
    let installed = InstalledPackage {
        name: entry.name.clone(),
        version: entry.version.clone(),
        release: entry.release,
        install_date: chrono::Utc::now().to_rfc3339(),
        package_hash: entry.sha256.clone(),
        files: file_list,
        depends: entry.depends.runtime.clone(),
        origin: entry.filename.clone(),
    };

    db.add(installed);
    db.save()?;

    info!(package = name, "installed successfully");
    Ok(())
}

/// Find a package across all configured repositories.
fn find_package_in_repos(
    name: &str,
    config: &LysinConfig,
) -> Result<(ribosome_repository::IndexEntry, Repository)> {
    for repo_path in &config.repositories {
        let path = std::path::Path::new(repo_path);
        if !path.exists() {
            warn!(path = %path.display(), "repository path does not exist, skipping");
            continue;
        }
        let repo = Repository::open(path)?;
        let index = repo.load_index()?;
        if let Some(entry) = index.find(name) {
            return Ok((entry.clone(), repo));
        }
    }
    bail!("package '{}' not found in any configured repository", name)
}

fn hash_file(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    std::io::copy(&mut file, &mut hasher)?;
    let result = hasher.finalize();
    Ok(format!("sha256:{result:x}"))
}
