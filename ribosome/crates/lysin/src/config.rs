use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Lysin configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LysinConfig {
    /// Root directory for installations (default: `/`).
    pub root: PathBuf,
    /// Path to the local package database directory (default: `<root>/var/lib/lysin`).
    pub db_path: PathBuf,
    /// Path to the local package cache (default: `<root>/var/cache/lysin`).
    pub cache_path: PathBuf,
    /// List of repository roots to search for packages.
    pub repositories: Vec<String>,
}

impl LysinConfig {
    /// Create a default config for development/testing.
    pub fn dev(root: &std::path::Path) -> Self {
        Self {
            root: root.to_path_buf(),
            db_path: root.join("var/lib/lysin"),
            cache_path: root.join("var/cache/lysin"),
            repositories: vec![],
        }
    }

    /// Load config from a TOML file. Returns default if file doesn't exist.
    pub fn load_or_default(root: &std::path::Path) -> Self {
        let config_path = root.join("etc/lysin/config.toml");
        Self::load_from_file(&config_path, root).unwrap_or_else(|_| Self::dev(root))
    }

    /// Load config from a specific TOML file.
    pub fn load_from_file(
        config_path: &std::path::Path,
        fallback_root: &std::path::Path,
    ) -> Result<Self> {
        if !config_path.exists() {
            return Ok(Self::dev(fallback_root));
        }

        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("reading config file {}", config_path.display()))?;

        let raw: RawConfig = toml::from_str(&content)
            .with_context(|| format!("parsing config file {}", config_path.display()))?;

        Ok(Self::from_raw(raw, fallback_root))
    }

    /// Convert raw TOML config to LysinConfig with defaults filled in.
    fn from_raw(raw: RawConfig, fallback_root: &std::path::Path) -> Self {
        Self {
            root: raw.root.unwrap_or_else(|| fallback_root.to_path_buf()),
            db_path: raw
                .db_path
                .unwrap_or_else(|| fallback_root.join("var/lib/lysin")),
            cache_path: raw
                .cache_path
                .unwrap_or_else(|| fallback_root.join("var/cache/lysin")),
            repositories: raw.repositories.unwrap_or_default(),
        }
    }

    /// Ensure all necessary directories exist.
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.db_path)
            .with_context(|| format!("creating db dir {}", self.db_path.display()))?;
        std::fs::create_dir_all(&self.cache_path)
            .with_context(|| format!("creating cache dir {}", self.cache_path.display()))?;
        Ok(())
    }
}

/// Raw TOML config structure with optional fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawConfig {
    #[serde(default)]
    root: Option<PathBuf>,
    #[serde(default)]
    db_path: Option<PathBuf>,
    #[serde(default)]
    cache_path: Option<PathBuf>,
    #[serde(default)]
    repositories: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dev_sets_correct_paths() {
        let config = LysinConfig::dev(std::path::Path::new("/mnt/test"));
        assert_eq!(config.root, std::path::PathBuf::from("/mnt/test"));
        assert_eq!(
            config.db_path,
            std::path::PathBuf::from("/mnt/test/var/lib/lysin")
        );
        assert_eq!(
            config.cache_path,
            std::path::PathBuf::from("/mnt/test/var/cache/lysin")
        );
        assert!(config.repositories.is_empty());
    }

    #[test]
    fn config_load_or_default_returns_dev_config() {
        let config = LysinConfig::load_or_default(std::path::Path::new("/"));
        assert_eq!(config.root, std::path::PathBuf::from("/"));
    }

    #[test]
    fn config_ensure_dirs_creates_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let config = LysinConfig::dev(tmp.path());

        assert!(!config.db_path.exists());
        assert!(!config.cache_path.exists());

        config.ensure_dirs().unwrap();

        assert!(config.db_path.exists());
        assert!(config.cache_path.exists());
    }

    #[test]
    fn config_ensure_dirs_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let config = LysinConfig::dev(tmp.path());

        config.ensure_dirs().unwrap();
        config.ensure_dirs().unwrap(); // Should not fail on second call

        assert!(config.db_path.exists());
    }

    #[test]
    fn config_load_from_file_reads_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("etc/lysin");
        std::fs::create_dir_all(&config_dir).unwrap();

        let config_content = r#"
root = "/custom/root"
db_path = "/custom/db"
cache_path = "/custom/cache"
repositories = ["https://repo.example.com"]
"#;
        std::fs::write(config_dir.join("config.toml"), config_content).unwrap();

        let config =
            LysinConfig::load_from_file(&config_dir.join("config.toml"), tmp.path()).unwrap();

        assert_eq!(config.root, std::path::PathBuf::from("/custom/root"));
        assert_eq!(config.db_path, std::path::PathBuf::from("/custom/db"));
        assert_eq!(config.cache_path, std::path::PathBuf::from("/custom/cache"));
        assert_eq!(config.repositories, vec!["https://repo.example.com"]);
    }

    #[test]
    fn config_load_from_file_partial_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("etc/lysin");
        std::fs::create_dir_all(&config_dir).unwrap();

        let config_content = r#"
repositories = ["repo1", "repo2"]
"#;
        std::fs::write(config_dir.join("config.toml"), config_content).unwrap();

        let config =
            LysinConfig::load_from_file(&config_dir.join("config.toml"), tmp.path()).unwrap();

        // Missing fields should use fallback_root defaults
        assert_eq!(config.root, tmp.path().to_path_buf());
        assert_eq!(config.db_path, tmp.path().join("var/lib/lysin"));
        assert_eq!(config.repositories, vec!["repo1", "repo2"]);
    }

    #[test]
    fn config_load_from_file_nonexistent_returns_dev() {
        let tmp = tempfile::tempdir().unwrap();
        let config =
            LysinConfig::load_from_file(&tmp.path().join("nonexistent.toml"), tmp.path()).unwrap();

        assert_eq!(config.root, tmp.path().to_path_buf());
    }
}
