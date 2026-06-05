use crate::config::LysinConfig;
use crate::db::LocalDb;
use anyhow::Result;

/// List all installed packages.
pub async fn list(config: &LysinConfig) -> Result<()> {
    let mut db = LocalDb::new(&config.db_path);
    db.load()?;

    if db.list().is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    println!("{:<30} {:<15} {:<5} Origin", "Name", "Version", "Rel");
    println!("{}", "-".repeat(75));
    for pkg in db.list() {
        println!(
            "{:<30} {:<15} {:<5} {}",
            pkg.name, pkg.version, pkg.release, pkg.origin
        );
    }
    println!("\n{} packages installed.", db.list().len());
    Ok(())
}
