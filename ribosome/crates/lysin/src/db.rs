use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// An installed package record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub release: u32,
    pub install_date: String,
    pub package_hash: String,
    pub files: Vec<String>,
    pub depends: Vec<String>,
    pub origin: String,
}

/// Local package database tracking installed packages.
pub struct LocalDb {
    path: PathBuf,
    packages: Vec<InstalledPackage>,
}

impl LocalDb {
    const DB_FILENAME: &'static str = "installed.db";

    pub fn new(root: &Path) -> Self {
        let path = root.join(Self::DB_FILENAME);
        Self {
            path,
            packages: Vec::new(),
        }
    }

    /// Load the database from disk.
    pub fn load(&mut self) -> Result<()> {
        if !self.path.exists() {
            self.packages = Vec::new();
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;

        self.packages = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| serde_json::from_str::<InstalledPackage>(line).ok())
            .collect();

        Ok(())
    }

    /// Save the database to disk.
    pub fn save(&self) -> Result<()> {
        let parent = self.path.parent().context("db path has no parent")?;
        std::fs::create_dir_all(parent)?;

        let tmp_path = self.path.with_extension("db.tmp");
        let mut content = String::new();
        for pkg in &self.packages {
            let json = serde_json::to_string(pkg)
                .with_context(|| format!("serializing package {}", pkg.name))?;
            content.push_str(&json);
            content.push('\n');
        }
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &self.path)?;

        Ok(())
    }

    /// Add an installed package.
    pub fn add(&mut self, package: InstalledPackage) {
        // Remove existing entry for the same package.
        self.packages.retain(|p| p.name != package.name);
        self.packages.push(package);
    }

    /// Remove a package by name. Returns the removed package if found.
    pub fn remove(&mut self, name: &str) -> Option<InstalledPackage> {
        let idx = self.packages.iter().position(|p| p.name == name)?;
        Some(self.packages.remove(idx))
    }

    /// Find a package by name.
    pub fn find(&self, name: &str) -> Option<&InstalledPackage> {
        self.packages.iter().find(|p| p.name == name)
    }

    /// List all installed packages.
    pub fn list(&self) -> &[InstalledPackage] {
        &self.packages
    }

    /// Check if a package is installed.
    pub fn is_installed(&self, name: &str) -> bool {
        self.packages.iter().any(|p| p.name == name)
    }

    /// Get the installed package names.
    pub fn installed_names(&self) -> Vec<&str> {
        self.packages.iter().map(|p| p.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile;

    fn test_pkg(name: &str) -> InstalledPackage {
        InstalledPackage {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            release: 1,
            install_date: "2026-06-04T00:00:00Z".to_string(),
            package_hash: "sha256:abc".to_string(),
            files: vec!["/usr/bin/test".to_string()],
            depends: vec![],
            origin: "core/test-1.0.0-1-x86_64.prot".to_string(),
        }
    }

    fn test_pkg_with_deps(name: &str, deps: Vec<&str>) -> InstalledPackage {
        InstalledPackage {
            depends: deps.into_iter().map(|d| d.to_string()).collect(),
            ..test_pkg(name)
        }
    }

    #[test]
    fn db_add_find_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();

        assert!(!db.is_installed("bash"));

        db.add(test_pkg("bash"));
        assert!(db.is_installed("bash"));

        let found = db.find("bash").unwrap();
        assert_eq!(found.version, "1.0.0");

        let removed = db.remove("bash").unwrap();
        assert_eq!(removed.name, "bash");
        assert!(!db.is_installed("bash"));
    }

    #[test]
    fn db_save_and_reload() {
        let tmp = tempfile::tempdir().unwrap();

        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();
        db.add(test_pkg("bash"));
        db.add(test_pkg("glibc"));
        db.save().unwrap();

        let mut db2 = LocalDb::new(tmp.path());
        db2.load().unwrap();
        assert_eq!(db2.list().len(), 2);
        assert!(db2.is_installed("bash"));
        assert!(db2.is_installed("glibc"));
    }

    #[test]
    fn db_replace_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();

        db.add(test_pkg("bash"));
        let mut updated = test_pkg("bash");
        updated.version = "2.0.0".to_string();
        db.add(updated);

        assert_eq!(db.list().len(), 1);
        assert_eq!(db.find("bash").unwrap().version, "2.0.0");
    }

    #[test]
    fn db_load_nonexistent_creates_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();

        assert!(db.list().is_empty());
        assert!(!db.is_installed("anything"));
        assert!(db.find("nothing").is_none());
    }

    #[test]
    fn db_remove_nonexistent_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();

        assert!(db.remove("ghost").is_none());
    }

    #[test]
    fn db_find_nonexistent_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();
        db.add(test_pkg("bash"));

        assert!(db.find("gcc").is_none());
    }

    #[test]
    fn db_installed_names() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();

        db.add(test_pkg("bash"));
        db.add(test_pkg("gcc"));
        db.add(test_pkg("glibc"));

        let mut names = db.installed_names();
        names.sort();
        assert_eq!(names, vec!["bash", "gcc", "glibc"]);
    }

    #[test]
    fn db_handles_corrupted_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("installed.db");
        // Write a mix of valid and invalid JSON lines
        std::fs::write(
            &db_path,
            "this is not json\n{\"name\":\"bash\",\"version\":\"1.0\",\"release\":1,\"install_date\":\"\",\"package_hash\":\"\",\"files\":[],\"depends\":[],\"origin\":\"\"}\n\n",
        ).unwrap();

        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();

        // Corrupted lines should be silently skipped
        assert_eq!(db.list().len(), 1);
        assert!(db.is_installed("bash"));
    }

    #[test]
    fn db_preserves_package_with_deps() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = LocalDb::new(tmp.path());
        db.load().unwrap();

        let pkg = test_pkg_with_deps("app", vec!["liba", "libb"]);
        db.add(pkg);
        db.save().unwrap();

        let mut db2 = LocalDb::new(tmp.path());
        db2.load().unwrap();
        let loaded = db2.find("app").unwrap();
        assert_eq!(loaded.depends, vec!["liba", "libb"]);
    }
}
