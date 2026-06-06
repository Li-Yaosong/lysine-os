use anyhow::{bail, Context, Result};
use tracing::info;

use crate::config::LysinConfig;
use crate::db::LocalDb;

/// Remove an installed package.
pub async fn remove(name: &str, config: &LysinConfig, force: bool) -> Result<()> {
    let mut db = LocalDb::new(&config.db_path);
    db.load()?;

    let pkg = db
        .find(name)
        .context(format!("package '{name}' is not installed"))?;

    info!(package = name, "removing");
    println!("Removing {}-{}...", pkg.name, pkg.version);

    // Check for reverse dependencies.
    let installed_names: Vec<&str> = db.installed_names();
    let dependents: Vec<&str> = installed_names
        .iter()
        .filter(|&&installed_name| {
            if installed_name == name {
                return false;
            }
            let installed_pkg = db.find(installed_name).unwrap();
            installed_pkg.depends.iter().any(|dep| {
                let dep_name = dep.split_whitespace().next().unwrap_or(dep);
                dep_name == name
            })
        })
        .copied()
        .collect();

    if !dependents.is_empty() && !force {
        bail!(
            "cannot remove '{}': the following installed packages depend on it: {}\nUse --force to override.",
            name,
            dependents.join(", ")
        );
    }

    // Remove files.
    let mut removed = 0;
    let mut failed = 0;
    for file_path in &pkg.files {
        let path = std::path::Path::new(file_path);
        if path.exists() {
            match std::fs::remove_file(path) {
                Ok(()) => removed += 1,
                Err(e) => {
                    failed += 1;
                    info!(path = %file_path, error = %e, "failed to remove file");
                }
            }
        }
    }

    // Remove from database.
    db.remove(name);
    db.save()?;

    println!(
        "Removed {} ({} files deleted, {} failed)",
        name, removed, failed
    );
    Ok(())
}
