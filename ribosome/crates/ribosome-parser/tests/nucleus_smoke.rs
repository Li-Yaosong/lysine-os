use std::path::Path;

use ribosome_parser::parse_mrna_file;

/// Smoke test: all nucleus mRNA files should parse and validate successfully.
#[test]
fn nucleus_core_packages_all_parse() {
    let nucleus_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate dir has parent")
        .parent()
        .expect("crates dir has parent")
        .parent()
        .expect("ribosome dir has parent")
        .join("nucleus")
        .join("core");

    if !nucleus_dir.exists() {
        eprintln!("nucleus/core/ not found, skipping nucleus smoke test");
        return;
    }

    let mut count = 0;
    for entry in walkdir::WalkDir::new(&nucleus_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("mRNA") {
            continue;
        }
        let label = path.file_name().unwrap().to_string_lossy().to_string();
        let mrna = parse_mrna_file(path)
            .unwrap_or_else(|e| panic!("{label}: expected to parse successfully, got: {e}"));
        assert!(!mrna.name.is_empty(), "{label}: name is empty");
        assert_eq!(mrna.api_version, 1, "{label}: unexpected api-version");
        assert!(mrna.release >= 1, "{label}: release should be >= 1");
        assert!(!mrna.sources.is_empty(), "{label}: no sources");
        count += 1;
    }
    assert!(
        count >= 70,
        "expected at least 70 nucleus mRNA files, got {count}"
    );
}
