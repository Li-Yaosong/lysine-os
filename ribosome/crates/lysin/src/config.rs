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
        // For Sprint 2, we use defaults. Config file loading will be added later.
        Self::dev(root)
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
