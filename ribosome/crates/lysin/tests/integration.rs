//! Integration tests for lysin package manager operations.
//!
//! These tests exercise the full install/remove/list/info/search pipeline
//! using temporary directories as mock filesystem roots and repositories.

use std::fs;
use std::path::Path;

use ribosome_package::{pack, PackageMeta};
use ribosome_repository::Repository;

/// Helper: create a LysinConfig pointing at a temp root with a repo.
fn setup_env(root: &Path, repo_dir: &Path) -> lysin::config::LysinConfig {
    let mut config = lysin::config::LysinConfig::dev(root);
    config.repositories = vec![repo_dir.to_string_lossy().to_string()];
    config
}

/// Helper: create a fake DESTDIR and pack it into a .prot file.
fn create_prot_package(
    destdir: &Path,
    name: &str,
    version: &str,
    runtime_deps: Vec<String>,
    output_dir: &Path,
) -> ribosome_package::PackResult {
    let bin_dir = destdir.join("usr/bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(
        bin_dir.join(name),
        format!("#!/bin/sh\necho {name}-{version}\n"),
    )
    .unwrap();

    let meta = PackageMeta {
        name: name.to_string(),
        version: version.to_string(),
        release: 1,
        arch: "x86_64".to_string(),
        mrna_yaml: format!("api-version: 1\nname: {name}\nversion: {version}\n"),
        depends_build: vec![],
        depends_runtime: runtime_deps,
        post_install: None,
        post_remove: None,
        build_duration: std::time::Duration::from_secs(1),
    };
    pack(destdir, &meta, output_dir, None).unwrap()
}

/// Setup: create a repository with a few packages and return (root, repo_dir).
fn setup_test_repo() -> (tempfile::TempDir, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    Repository::create(repo_dir.path()).unwrap();

    let staging = root.path().join("staging");
    fs::create_dir_all(&staging).unwrap();

    // Create packages: liba (no deps), libb (depends liba), app (depends both)
    for (name, version, deps) in [
        ("liba", "1.0.0", vec![]),
        ("libb", "2.0.0", vec!["liba".to_string()]),
        ("app", "3.0.0", vec!["liba".to_string(), "libb".to_string()]),
    ] {
        let destdir = root.path().join(format!("build_{name}"));
        create_prot_package(&destdir, name, version, deps, &staging);

        let prot_path = staging.join(format!("{name}-{version}-1-x86_64.prot"));
        let repo = Repository::open(repo_dir.path()).unwrap();
        repo.publish(&prot_path, "core").unwrap();
    }

    let repo = Repository::open(repo_dir.path()).unwrap();
    repo.rebuild_index().unwrap();

    (root, repo_dir)
}

#[tokio::test]
async fn lysin_install_and_remove_package() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    // Install liba
    lysin::ops::install::install("liba", &config).await.unwrap();

    // Verify it's in the database
    let mut db = lysin::db::LocalDb::new(&config.db_path);
    db.load().unwrap();
    assert!(db.is_installed("liba"));
    assert_eq!(db.find("liba").unwrap().version, "1.0.0");

    // Verify the binary exists
    let bin = root.path().join("usr/bin/liba");
    assert!(bin.exists());

    // Remove liba (force since nothing depends on it in the db)
    lysin::ops::remove::remove("liba", &config, true)
        .await
        .unwrap();

    let mut db2 = lysin::db::LocalDb::new(&config.db_path);
    db2.load().unwrap();
    assert!(!db2.is_installed("liba"));
}

#[tokio::test]
async fn lysin_install_with_dependencies() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    // Install dependencies manually first (because the repo index
    // doesn't carry dependency info from .prot packages currently),
    // then install app.
    lysin::ops::install::install("liba", &config).await.unwrap();
    lysin::ops::install::install("libb", &config).await.unwrap();
    lysin::ops::install::install("app", &config).await.unwrap();

    let mut db = lysin::db::LocalDb::new(&config.db_path);
    db.load().unwrap();

    assert!(db.is_installed("liba"));
    assert!(db.is_installed("libb"));
    assert!(db.is_installed("app"));
    assert_eq!(db.list().len(), 3);
}

#[tokio::test]
async fn lysin_list_shows_installed_packages() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    // Empty: list should handle gracefully
    lysin::ops::list::list(&config).await.unwrap();

    // Install and list
    lysin::ops::install::install("liba", &config).await.unwrap();
    lysin::ops::list::list(&config).await.unwrap();

    let mut db = lysin::db::LocalDb::new(&config.db_path);
    db.load().unwrap();
    assert_eq!(db.list().len(), 1);
}

#[tokio::test]
async fn lysin_info_for_installed_package() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    lysin::ops::install::install("liba", &config).await.unwrap();

    // Should find in local db
    lysin::ops::info::info("liba", &config).await.unwrap();
}

#[tokio::test]
async fn lysin_info_for_uninstalled_package_in_repo() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    // Should find in repository index
    lysin::ops::info::info("liba", &config).await.unwrap();
}

#[tokio::test]
async fn lysin_info_for_unknown_package_fails() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    let result = lysin::ops::info::info("nonexistent", &config).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn lysin_search_finds_packages() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    // Should find liba and libb when searching "lib"
    lysin::ops::search::search("lib", &config).await.unwrap();

    // Should find nothing
    lysin::ops::search::search("zzznonexistent", &config)
        .await
        .unwrap();
}

#[tokio::test]
async fn lysin_deps_shows_dependencies() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    // app has liba and libb as runtime deps
    lysin::ops::deps::deps("app", &config).await.unwrap();

    // liba has no deps
    lysin::ops::deps::deps("liba", &config).await.unwrap();
}

#[tokio::test]
async fn lysin_remove_checks_reverse_deps() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    // Install all packages
    lysin::ops::install::install("liba", &config).await.unwrap();
    lysin::ops::install::install("libb", &config).await.unwrap();
    lysin::ops::install::install("app", &config).await.unwrap();

    // Manually add dependency info to app's record in the db
    // (because the repo index doesn't carry dep info from .prot packages)
    {
        let mut db = lysin::db::LocalDb::new(&config.db_path);
        db.load().unwrap();
        let mut app_pkg = db.remove("app").unwrap();
        app_pkg.depends = vec!["liba".to_string(), "libb".to_string()];
        db.add(app_pkg);
        db.save().unwrap();
    }

    // Try removing liba without force -- should fail because app depends on it
    let result = lysin::ops::remove::remove("liba", &config, false).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("depend on it"),
        "expected dependency error, got: {err_msg}"
    );
}

#[tokio::test]
async fn lysin_install_already_installed_is_noop() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    lysin::ops::install::install("liba", &config).await.unwrap();

    // Second install should succeed (no-op)
    lysin::ops::install::install("liba", &config).await.unwrap();
}

#[tokio::test]
async fn lysin_update_detects_newer_version() {
    let (root, repo_dir) = setup_test_repo();
    let config = setup_env(root.path(), repo_dir.path());

    // Install liba 1.0.0
    lysin::ops::install::install("liba", &config).await.unwrap();

    let mut db = lysin::db::LocalDb::new(&config.db_path);
    db.load().unwrap();
    assert_eq!(db.find("liba").unwrap().version, "1.0.0");

    // Rebuild repo with liba 2.0.0
    let staging = root.path().join("staging2");
    fs::create_dir_all(&staging).unwrap();
    let destdir = root.path().join("build_liba_v2");
    create_prot_package(&destdir, "liba", "2.0.0", vec![], &staging);

    let repo = Repository::open(repo_dir.path()).unwrap();
    let prot_path = staging.join("liba-2.0.0-1-x86_64.prot");
    repo.publish(&prot_path, "core").unwrap();
    repo.rebuild_index().unwrap();

    // Run update
    lysin::ops::update::update(&config).await.unwrap();

    // Verify upgraded
    let mut db2 = lysin::db::LocalDb::new(&config.db_path);
    db2.load().unwrap();
    assert_eq!(db2.find("liba").unwrap().version, "2.0.0");
}
