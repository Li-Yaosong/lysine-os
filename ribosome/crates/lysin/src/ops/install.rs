use anyhow::{bail, Context, Result};
use ribosome_package::unpack;
use ribosome_repository::Repository;
use tracing::{info, warn};

use super::hash_file;
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
    let (entry, _repo) = find_package_in_repos(name, config)?;

    info!(package = name, version = %entry.version, "installing");
    println!("Installing {}-{}...", entry.name, entry.version);

    // Install dependencies first.
    for dep in &entry.depends.runtime {
        let dep_name = dep.split_whitespace().next().unwrap_or(dep);
        if !db.is_installed(dep_name) {
            info!(dependency = dep_name, "installing dependency");
            println!("  Installing dependency: {}", dep_name);
            install_single(dep_name, config, &mut db).await?;
        }
    }

    // Install the package itself.
    install_single(name, config, &mut db).await?;

    println!("Done.");
    Ok(())
}

/// Install a single package (no dependency resolution).
/// Searches across all configured repositories to find the package.
async fn install_single(name: &str, config: &LysinConfig, db: &mut LocalDb) -> Result<()> {
    let (entry, repo) = find_package_in_repos(name, config)?;

    // Try to resolve the .prot file from the vacuole CAS cache first.
    let prot_path = resolve_prot_path(&entry, &repo, config)?;

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
    let extracted =
        unpack(&prot_path, &config.root).context(format!("unpacking {}", prot_path.display()))?;

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

/// Resolve the .prot file path, preferring the vacuole CAS cache when available.
///
/// If the package is already stored in the local CAS, returns the cached path.
/// Otherwise falls back to the repository's local file.
fn resolve_prot_path(
    entry: &ribosome_repository::IndexEntry,
    repo: &Repository,
    config: &LysinConfig,
) -> Result<std::path::PathBuf> {
    let vacuole_path = config.cache_path.join("vacuole");
    if vacuole_path.exists() {
        match ribosome_store::VacuoleStore::open(&vacuole_path) {
            Ok(store) => {
                let ref_name = format!(
                    "{}-{}-{}-{}",
                    entry.name, entry.version, entry.release, entry.arch
                );
                if let Ok(Some(digest)) =
                    store.resolve_ref(ribosome_store::refs::NS_PACKAGES, &ref_name)
                {
                    if let Ok(Some(handle)) = store.get(&digest) {
                        info!(package = %entry.name, "resolved from vacuole CAS cache");
                        return Ok(handle.path().to_path_buf());
                    }
                }
            }
            Err(e) => {
                warn!(
                    error = %e,
                    path = %vacuole_path.display(),
                    "failed to open vacuole store, falling back to repository"
                );
            }
        }
    }

    // Fallback: use the repository's local .prot file
    let prot_path = repo.root.join(&entry.filename);
    if !prot_path.exists() {
        bail!(
            "package file not found: {} — run 'ribosome repo reindex' to rebuild the index",
            prot_path.display()
        );
    }
    Ok(prot_path)
}
