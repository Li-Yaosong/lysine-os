use anyhow::{bail, Context, Result};
use ribosome_package::unpack;
use ribosome_repository::Repository;
use tracing::{info, warn};

use crate::config::LysinConfig;
use crate::db::{InstalledPackage, LocalDb};

/// Update all installed packages to the latest versions found in repositories.
pub async fn update(config: &LysinConfig) -> Result<()> {
    config.ensure_dirs()?;

    let mut db = LocalDb::new(&config.db_path);
    db.load()?;

    let installed = db.list();
    if installed.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    info!(count = installed.len(), "checking for updates");

    // Collect packages that have updates available.
    let mut to_update: Vec<(
        InstalledPackage,
        ribosome_repository::IndexEntry,
        Repository,
    )> = Vec::new();

    for pkg in installed {
        match find_newer_version(&pkg.name, &pkg.version, config) {
            Ok(Some((entry, repo))) => {
                println!(
                    "  {} {} -> {} (release {} -> {})",
                    pkg.name, pkg.version, entry.version, pkg.release, entry.release
                );
                to_update.push((pkg.clone(), entry, repo));
            }
            Ok(None) => {
                // Already up-to-date.
            }
            Err(e) => {
                warn!(package = %pkg.name, error = %e, "skipping update check");
            }
        }
    }

    if to_update.is_empty() {
        println!("All {} packages are up to date.", installed.len());
        return Ok(());
    }

    println!(
        "\n{} package(s) can be updated. Proceeding...",
        to_update.len()
    );

    // Apply updates one by one.
    let mut updated_count = 0;
    let mut failed_count = 0;

    for (old_pkg, new_entry, repo) in &to_update {
        info!(package = %old_pkg.name, old = %old_pkg.version, new = %new_entry.version, "updating");
        println!(
            "Updating {} from {} to {}...",
            old_pkg.name, old_pkg.version, new_entry.version
        );

        match update_single(old_pkg, new_entry, repo, config, &mut db).await {
            Ok(()) => {
                updated_count += 1;
                println!("  Done.");
            }
            Err(e) => {
                failed_count += 1;
                warn!(package = %old_pkg.name, error = %e, "update failed");
                println!("  FAILED: {e:#}");
            }
        }
    }

    println!("\nUpdate complete: {updated_count} updated, {failed_count} failed.");
    Ok(())
}

/// Find a newer version of a package in configured repositories.
/// Returns the index entry and repository if a newer version exists.
fn find_newer_version(
    name: &str,
    current_version: &str,
    config: &LysinConfig,
) -> Result<Option<(ribosome_repository::IndexEntry, Repository)>> {
    for repo_path in &config.repositories {
        let path = std::path::Path::new(repo_path);
        if !path.exists() {
            continue;
        }
        let repo = Repository::open(path)?;
        let index = repo.load_index()?;

        if let Some(entry) = index.find(name) {
            // Compare versions: prefer higher version string (lexicographic),
            // then higher release number.
            if entry.version.as_str() > current_version {
                return Ok(Some((entry.clone(), repo)));
            }
            if entry.version == current_version && entry.release > 0 {
                // Same version but different release (rebuild).
                // The index always holds the latest, so if the release differs,
                // it's an update. We check the installed DB for the actual release.
                // For now, this branch is a placeholder for release-based updates.
            }
        }
    }
    Ok(None)
}

/// Update a single package: remove old files, install new version.
async fn update_single(
    old_pkg: &InstalledPackage,
    new_entry: &ribosome_repository::IndexEntry,
    repo: &Repository,
    config: &LysinConfig,
    db: &mut LocalDb,
) -> Result<()> {
    let prot_path = repo.root.join(&new_entry.filename);
    if !prot_path.exists() {
        bail!(
            "package file not found: {} — run 'ribosome repo reindex'",
            prot_path.display()
        );
    }

    // Verify hash of the new package.
    let actual_hash = hash_file(&prot_path)?;
    if actual_hash != new_entry.sha256 {
        bail!(
            "hash mismatch for {}: expected {}, got {}",
            prot_path.display(),
            new_entry.sha256,
            actual_hash
        );
    }

    // Remove old files.
    let mut removed = 0;
    for file_path in &old_pkg.files {
        let path = std::path::Path::new(file_path);
        if path.exists() && std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    info!(package = %old_pkg.name, files_removed = removed, "removed old files");

    // Extract new version.
    let extracted =
        unpack(&prot_path, &config.root).context(format!("unpacking {}", prot_path.display()))?;

    let file_list: Vec<String> = extracted
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    // Update database entry.
    let updated_pkg = InstalledPackage {
        name: new_entry.name.clone(),
        version: new_entry.version.clone(),
        release: new_entry.release,
        install_date: chrono::Utc::now().to_rfc3339(),
        package_hash: new_entry.sha256.clone(),
        files: file_list,
        depends: new_entry.depends.runtime.clone(),
        origin: new_entry.filename.clone(),
    };

    db.add(updated_pkg);
    db.save()?;

    info!(package = %old_pkg.name, "updated successfully");
    Ok(())
}

fn hash_file(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    std::io::copy(&mut file, &mut hasher)?;
    let result = hasher.finalize();
    Ok(format!("sha256:{result:x}"))
}
