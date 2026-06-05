use anyhow::Result;
use ribosome_repository::PackageQuery;

use crate::config::LysinConfig;

/// Search for packages in configured repositories.
pub async fn search(keyword: &str, config: &LysinConfig) -> Result<()> {
    let mut total = 0;

    for repo_path in &config.repositories {
        let path = std::path::Path::new(repo_path);
        if !path.exists() {
            continue;
        }
        let repo = ribosome_repository::Repository::open(path)?;
        let index = repo.load_index()?;
        let query = PackageQuery::new(&index);

        let results = query.search(keyword);
        for info in &results {
            let e = &info.entry;
            println!("{:<30} {:<15} {}", e.name, e.version, e.description);
        }
        total += results.len();
    }

    if total == 0 {
        println!("No packages found matching '{}'.", keyword);
    } else {
        println!("\n{} package(s) found.", total);
    }
    Ok(())
}
