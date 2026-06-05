//! End-to-end integration test for the Ribosome build pipeline.
//!
//! Tests the full chain: mRNA parse -> dep graph -> pack -> repo -> install/unpack.
//! Skips actual build execution (uses pre-created DESTDIR files).

use std::fs;
use std::path::Path;

use ribosome_deps::DependencyGraph;
use ribosome_package::{pack, unpack, PackageMeta};
use ribosome_parser::parse_mrna;
use ribosome_repository::Repository;

/// Create a test mRNA file.
fn create_test_mrna(dir: &Path, name: &str, version: &str, runtime_deps: &[&str]) {
    let pkg_dir = dir.join(name);
    fs::create_dir_all(&pkg_dir).unwrap();

    let deps_yaml = if runtime_deps.is_empty() {
        "".to_string()
    } else {
        let dep_lines: String = runtime_deps
            .iter()
            .map(|d| format!("    - {}\n", d))
            .collect();
        format!("depends:\n  runtime:\n{}\n", dep_lines)
    };

    let content = format!(
        r#"api-version: 1
name: {name}
version: {version}
release: 1

description: Test package {name}
license: MIT

{deps_yaml}
sources:
  - url: https://example.com/{name}-{version}.tar.gz
    hash: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa

build:
  install: |
    mkdir -p "$DESTDIR/usr/bin"
    echo '#!/bin/sh' > "$DESTDIR/usr/bin/{name}"
    echo 'echo {name}-{version}' >> "$DESTDIR/usr/bin/{name}"
"#,
        name = name,
        version = version,
        deps_yaml = deps_yaml
    );

    let mrna_path = pkg_dir.join(format!("{}.mRNA", version));
    fs::write(&mrna_path, content).unwrap();
}

/// Create a fake DESTDIR with a binary file.
fn create_fake_destdir(destdir: &Path, name: &str, version: &str) {
    let bin_dir = destdir.join("usr/bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join(name);
    fs::write(&script, format!("#!/bin/sh\necho {name}-{version}\n")).unwrap();
}

/// Create a .prot package from a fake DESTDIR.
fn create_prot_package(
    destdir: &Path,
    name: &str,
    version: &str,
    mrna_yaml: &str,
    runtime_deps: Vec<String>,
    output_dir: &Path,
) -> ribosome_package::PackResult {
    let meta = PackageMeta {
        name: name.to_string(),
        version: version.to_string(),
        release: 1,
        arch: "x86_64".to_string(),
        mrna_yaml: mrna_yaml.to_string(),
        depends_build: vec![],
        depends_runtime: runtime_deps,
        post_install: None,
        post_remove: None,
        build_duration: std::time::Duration::from_secs(1),
    };
    pack(destdir, &meta, output_dir).unwrap()
}

#[test]
fn e2e_parse_depgraph_pack_repo_unpack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // --- Phase 1: Create test mRNA files ---
    let mrna_dir = root.join("nucleus/core");
    fs::create_dir_all(&mrna_dir).unwrap();

    create_test_mrna(&mrna_dir, "liba", "1.0.0", &[]);
    create_test_mrna(&mrna_dir, "libb", "2.0.0", &["liba"]);
    create_test_mrna(&mrna_dir, "app", "3.0.0", &["liba", "libb"]);

    // --- Phase 2: Parse mRNA files ---
    let liba_mrna_content = fs::read_to_string(mrna_dir.join("liba/1.0.0.mRNA")).unwrap();
    let liba_mrna = parse_mrna(&liba_mrna_content).expect("liba mRNA should parse");
    assert_eq!(liba_mrna.name, "liba");
    assert_eq!(liba_mrna.version, "1.0.0");

    let libb_mrna_content = fs::read_to_string(mrna_dir.join("libb/2.0.0.mRNA")).unwrap();
    let libb_mrna = parse_mrna(&libb_mrna_content).expect("libb mRNA should parse");
    assert_eq!(libb_mrna.name, "libb");

    let app_mrna_content = fs::read_to_string(mrna_dir.join("app/3.0.0.mRNA")).unwrap();
    let app_mrna = parse_mrna(&app_mrna_content).expect("app mRNA should parse");
    assert_eq!(app_mrna.name, "app");

    // --- Phase 3: Build dependency graph ---
    let mut graph = DependencyGraph::new();
    graph.load_mrna_directory(&mrna_dir).expect("loading mrna dir");

    assert!(!graph.has_cycle());
    let mut order = graph.topological_sort();
    order.reverse(); // Reverse to get dependency-first order
    assert_eq!(order.len(), 3);

    // Verify order: liba (no deps) -> libb (depends liba) -> app (depends both)
    let liba_pos = order.iter().position(|n| n == "liba").unwrap();
    let libb_pos = order.iter().position(|n| n == "libb").unwrap();
    let app_pos = order.iter().position(|n| n == "app").unwrap();
    assert!(liba_pos < libb_pos, "liba should come before libb");
    assert!(libb_pos < app_pos, "libb should come before app");

    // --- Phase 4: Create .prot packages ---
    let repo_dir = root.join("repo");
    let repo = Repository::create(&repo_dir).expect("create repository");

    let build_dir = root.join("build");
    let prot_staging = root.join("prot-staging");
    fs::create_dir_all(&build_dir).unwrap();
    fs::create_dir_all(&prot_staging).unwrap();

    // Create fake DESTDIRs and packages
    for (name, version, _mrna, deps) in [
        ("liba", "1.0.0", &liba_mrna, vec![]),
        ("libb", "2.0.0", &libb_mrna, vec!["liba".to_string()]),
        ("app", "3.0.0", &app_mrna, vec!["liba".to_string(), "libb".to_string()]),
    ] {
        let destdir = build_dir.join(format!("{}_pkg", name));
        create_fake_destdir(&destdir, name, version);

        let prot_result = create_prot_package(
            &destdir,
            name,
            version,
            &fs::read_to_string(mrna_dir.join(format!("{}/{version}.mRNA", name))).unwrap(),
            deps,
            &prot_staging, // Output to staging, not directly to repo
        );

        // Publish to repository (copies from staging to repo/core)
        repo.publish(&prot_result.path, "core").expect("publish");
    }

    // --- Phase 5: Rebuild repository index ---
    let repo = Repository::open(&repo_dir).expect("open repo");
    repo.rebuild_index().expect("rebuild index");

    let index = repo.load_index().expect("load index");
    assert_eq!(index.len(), 3);

    // --- Phase 6: Query index ---
    let liba_entry = index.find("liba").expect("liba in index");
    assert_eq!(liba_entry.version, "1.0.0");

    let app_entry = index.find("app").expect("app in index");
    assert_eq!(app_entry.version, "3.0.0");
    // Note: depends.runtime is not populated by publish() currently.
    // It would need to be extracted from the .prot package's META/depends.txt.

    // --- Phase 7: Install (unpack) packages ---
    let install_root = root.join("install");
    fs::create_dir_all(&install_root).unwrap();

    // Install in correct order (liba -> libb -> app)
    for name in ["liba", "libb", "app"] {
        let entry = index.find(name).expect(&format!("{name} in index"));
        let prot_path = repo_dir.join(&entry.filename);
        assert!(prot_path.exists(), "prot file should exist: {:?}", prot_path);

        let extracted = unpack(&prot_path, &install_root).expect(&format!("unpack {name}"));
        assert!(!extracted.is_empty(), "{name} should have files");

        // Verify binary exists
        let bin_path = install_root.join(format!("usr/bin/{name}"));
        assert!(bin_path.exists(), "{name} binary should exist");
    }

    // Verify all binaries work
    for (name, version) in [("liba", "1.0.0"), ("libb", "2.0.0"), ("app", "3.0.0")] {
        let bin_path = install_root.join(format!("usr/bin/{name}"));
        let content = fs::read_to_string(&bin_path).unwrap();
        assert!(content.contains(&format!("{name}-{version}")));
    }

    println!("E2E integration test passed!");
}