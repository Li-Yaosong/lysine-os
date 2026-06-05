use anyhow::{bail, Result};

use crate::config::LysinConfig;
use crate::db::LocalDb;
use ribosome_repository::PackageQuery;

/// Show package information.
pub async fn info(name: &str, config: &LysinConfig) -> Result<()> {
    // First check local database.
    let mut db = LocalDb::new(&config.db_path);
    db.load()?;

    if let Some(pkg) = db.find(name) {
        println!("Package: {} (installed)", pkg.name);
        println!("Version: {}-{}", pkg.version, pkg.release);
        println!("Installed: {}", pkg.install_date);
        println!("Origin: {}", pkg.origin);
        println!("Hash: {}", pkg.package_hash);
        println!("Files: {}", pkg.files.len());
        if !pkg.depends.is_empty() {
            println!("Depends: {}", pkg.depends.join(", "));
        }
        return Ok(());
    }

    // Check repositories.
    for repo_path in &config.repositories {
        let path = std::path::Path::new(repo_path);
        if !path.exists() {
            continue;
        }
        let repo = ribosome_repository::Repository::open(path)?;
        let index = repo.load_index()?;
        let query = PackageQuery::new(&index);

        if let Some(info) = query.info(name) {
            println!("{}", info.to_summary());
            return Ok(());
        }
    }

    bail!("package '{}' not found (not installed, not in any repository)", name)
}
