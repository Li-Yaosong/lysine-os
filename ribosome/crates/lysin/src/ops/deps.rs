use anyhow::{bail, Result};

use crate::config::LysinConfig;
use ribosome_repository::PackageQuery;

/// Show dependency tree for a package.
pub async fn deps(name: &str, config: &LysinConfig) -> Result<()> {
    for repo_path in &config.repositories {
        let path = std::path::Path::new(repo_path);
        if !path.exists() {
            continue;
        }
        let repo = ribosome_repository::Repository::open(path)?;
        let index = repo.load_index()?;
        let query = PackageQuery::new(&index);

        if let Some(info) = query.info(name) {
            println!("{}-{}", info.entry.name, info.entry.version);
            if info.entry.depends.runtime.is_empty() {
                println!("  (no runtime dependencies)");
            } else {
                for dep in &info.entry.depends.runtime {
                    println!("  ├── {}", dep);
                }
            }
            return Ok(());
        }
    }
    bail!("package '{}' not found in any repository", name)
}
