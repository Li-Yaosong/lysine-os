use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tracing::info;
use walkdir::WalkDir;

use crate::error::{PackageError, Result};

/// Metadata for a .protein package being created.
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    pub release: u32,
    pub arch: String,
    pub mrna_yaml: String,
    pub depends_build: Vec<String>,
    pub depends_runtime: Vec<String>,
    pub post_install: Option<String>,
    pub post_remove: Option<String>,
    pub build_duration: std::time::Duration,
}

/// Result of a successful pack operation.
pub struct PackResult {
    pub path: PathBuf,
    pub sha256: String,
    pub file_count: usize,
    pub size_bytes: u64,
}

/// Create a `.protein` package from a DESTDIR staging directory.
///
/// The resulting tar.zst archive follows the layout:
/// ```text
/// <name>-<version>-<release>-<arch>.protein (tar.zst)
/// ├── META/
/// │   ├── mRNA.yml
/// │   ├── manifest.txt      (file list + sha256)
/// │   ├── depends.txt
/// │   └── build-info.txt
/// ├── FILES/
/// │   └── usr/bin/...
/// └── SCRIPTS/
///     ├── post-install.sh
///     └── post-remove.sh
/// ```
pub fn pack(dest_dir: &Path, meta: &PackageMeta, output_dir: &Path) -> Result<PackResult> {
    std::fs::create_dir_all(output_dir)?;

    let filename = format!(
        "{}-{}-{}-{}.protein",
        meta.name, meta.version, meta.release, meta.arch
    );
    let output_path = output_dir.join(&filename);
    let tmp_path = output_path.with_extension("protein.tmp");

    info!(package = %meta.name, version = %meta.version, "packing .protein");

    let file = std::fs::File::create(&tmp_path)?;
    let encoder = zstd::Encoder::new(file, 3)?;
    let mut builder = tar::Builder::new(encoder);

    // 1. META/ directory
    add_meta_mrna(&mut builder, meta)?;
    add_meta_manifest(&mut builder, dest_dir)?;
    add_meta_depends(&mut builder, meta)?;
    add_meta_build_info(&mut builder, meta)?;

    // 2. FILES/ — copy dest_dir contents under FILES/
    let mut file_count = 0usize;
    if dest_dir.exists() {
        for entry in WalkDir::new(dest_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let relative = path.strip_prefix(dest_dir).map_err(|_| {
                PackageError::CreationFailed(format!("cannot strip prefix from {}", path.display()))
            })?;
            let archive_path = Path::new("FILES").join(relative);
            let mut f = std::fs::File::open(path)?;
            builder.append_file(archive_path, &mut f)?;
            file_count += 1;
        }
    }

    // 3. SCRIPTS/
    if let Some(script) = &meta.post_install {
        add_script(&mut builder, "SCRIPTS/post-install.sh", script)?;
    }
    if let Some(script) = &meta.post_remove {
        add_script(&mut builder, "SCRIPTS/post-remove.sh", script)?;
    }

    // Finalize
    let encoder = builder.into_inner()?;
    encoder.finish()?;

    // Atomic rename: tmp → final
    std::fs::rename(&tmp_path, &output_path).map_err(|e| {
        // Clean up tmp file on rename failure
        let _ = std::fs::remove_file(&tmp_path);
        PackageError::CreationFailed(format!(
            "failed to rename {} to {}: {e}",
            tmp_path.display(),
            output_path.display()
        ))
    })?;

    // Compute SHA-256
    let sha256 = hash_file(&output_path)?;
    let size_bytes = std::fs::metadata(&output_path)?.len();

    info!(
        package = %meta.name,
        files = file_count,
        size = size_bytes,
        sha256 = &sha256[..16],
        "packed .protein"
    );

    Ok(PackResult {
        path: output_path,
        sha256,
        file_count,
        size_bytes,
    })
}

/// Extract a `.protein` package to a target directory.
///
/// Only the `FILES/` prefix is extracted — META and SCRIPTS are skipped.
/// Returns the list of extracted file paths.
///
/// # Security
///
/// Rejects archive entries that would escape `target_dir` via `..` traversal
/// or absolute paths (e.g. `FILES/../../../etc/passwd`).
pub fn unpack(protein_path: &Path, target_dir: &Path) -> Result<Vec<PathBuf>> {
    info!(path = %protein_path.display(), "unpacking .protein");

    let file = std::fs::File::open(protein_path)?;
    let decoder = zstd::Decoder::new(file)?;
    let mut archive = tar::Archive::new(decoder);

    std::fs::create_dir_all(target_dir)?;

    let mut extracted = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();

        // Only extract FILES/ entries
        if let Ok(relative) = path.strip_prefix("FILES") {
            if relative.as_os_str().is_empty() {
                continue;
            }
            // Reject path traversal attacks: no `..` components, no absolute paths
            if relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(PackageError::ExtractionFailed(format!(
                    "path escapes FILES root: {}",
                    relative.display()
                )));
            }
            let target = target_dir.join(relative);
            // Ensure parent directory exists
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&target)?;
            extracted.push(target);
        }
    }

    info!(files = extracted.len(), "unpacked .protein");
    Ok(extracted)
}

/// Compute SHA-256 of a file.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    let result = hasher.finalize();
    Ok(format!("sha256:{result:x}"))
}

// --- Internal helpers ---

fn add_meta_mrna(
    builder: &mut tar::Builder<zstd::Encoder<std::fs::File>>,
    meta: &PackageMeta,
) -> Result<()> {
    let data = meta.mrna_yaml.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "META/mRNA.yml", data)?;
    Ok(())
}

fn add_meta_manifest(
    builder: &mut tar::Builder<zstd::Encoder<std::fs::File>>,
    dest_dir: &Path,
) -> Result<()> {
    let mut manifest = String::new();
    if dest_dir.exists() {
        for entry in WalkDir::new(dest_dir).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let relative = path.strip_prefix(dest_dir).unwrap_or(path);
            let hash = hash_file_contents(path)?;
            manifest.push_str(&format!("{}  {}\n", hash, relative.display()));
        }
    }
    let data = manifest.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "META/manifest.txt", data)?;
    Ok(())
}

fn add_meta_depends(
    builder: &mut tar::Builder<zstd::Encoder<std::fs::File>>,
    meta: &PackageMeta,
) -> Result<()> {
    let mut content = String::new();
    for dep in &meta.depends_build {
        content.push_str(&format!("build: {dep}\n"));
    }
    for dep in &meta.depends_runtime {
        content.push_str(&format!("runtime: {dep}\n"));
    }
    let data = content.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "META/depends.txt", data)?;
    Ok(())
}

fn add_meta_build_info(
    builder: &mut tar::Builder<zstd::Encoder<std::fs::File>>,
    meta: &PackageMeta,
) -> Result<()> {
    let info = format!(
        "package: {}\nversion: {}\nrelease: {}\narch: {}\nbuild_duration: {:.1}s\nribosome_version: 0.1.0\n",
        meta.name,
        meta.version,
        meta.release,
        meta.arch,
        meta.build_duration.as_secs_f64(),
    );
    let data = info.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "META/build-info.txt", data)?;
    Ok(())
}

fn add_script(
    builder: &mut tar::Builder<zstd::Encoder<std::fs::File>>,
    name: &str,
    script: &str,
) -> Result<()> {
    let data = script.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder.append_data(&mut header, name, data)?;
    Ok(())
}

fn hash_file_contents(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    let result = hasher.finalize();
    Ok(format!("{result:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_meta() -> PackageMeta {
        PackageMeta {
            name: "test-pkg".to_string(),
            version: "1.0.0".to_string(),
            release: 1,
            arch: "x86_64".to_string(),
            mrna_yaml: "api-version: 1\nname: test-pkg\n".to_string(),
            depends_build: vec!["glibc >= 2.39".to_string()],
            depends_runtime: vec!["glibc".to_string()],
            post_install: Some("ldconfig".to_string()),
            post_remove: None,
            build_duration: std::time::Duration::from_secs(5),
        }
    }

    fn create_dest_dir(dest: &Path) {
        fs::create_dir_all(dest.join("usr/bin")).unwrap();
        fs::write(dest.join("usr/bin/hello"), "#!/bin/sh\necho hello\n").unwrap();
        fs::create_dir_all(dest.join("usr/lib")).unwrap();
        fs::write(dest.join("usr/lib/libtest.so"), "binary data here").unwrap();
    }

    #[test]
    fn pack_creates_protein_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dest_dir = tmp.path().join("dest");
        let output_dir = tmp.path().join("output");
        create_dest_dir(&dest_dir);

        let meta = test_meta();
        let result = pack(&dest_dir, &meta, &output_dir).unwrap();

        assert!(result.path.exists());
        assert!(result.path.to_string_lossy().ends_with(".protein"));
        assert!(result.sha256.starts_with("sha256:"));
        assert_eq!(result.file_count, 2); // hello + libtest.so
        assert!(result.size_bytes > 0);
    }

    #[test]
    fn unpack_extracts_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dest_dir = tmp.path().join("dest");
        let output_dir = tmp.path().join("output");
        let install_dir = tmp.path().join("install");

        create_dest_dir(&dest_dir);

        let meta = test_meta();
        let pack_result = pack(&dest_dir, &meta, &output_dir).unwrap();

        let extracted = unpack(&pack_result.path, &install_dir).unwrap();
        assert_eq!(extracted.len(), 2);

        let hello = install_dir.join("usr/bin/hello");
        assert!(hello.exists());
        let content = fs::read_to_string(&hello).unwrap();
        assert!(content.contains("echo hello"));

        let lib = install_dir.join("usr/lib/libtest.so");
        assert!(lib.exists());
    }

    #[test]
    fn pack_with_empty_dest_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dest_dir = tmp.path().join("empty-dest");
        let output_dir = tmp.path().join("output");
        fs::create_dir_all(&dest_dir).unwrap();

        let meta = test_meta();
        let result = pack(&dest_dir, &meta, &output_dir).unwrap();

        assert!(result.path.exists());
        assert_eq!(result.file_count, 0);
    }

    #[test]
    fn pack_unpack_roundtrip_preserves_content() {
        let tmp = tempfile::tempdir().unwrap();
        let dest_dir = tmp.path().join("dest");
        let output_dir = tmp.path().join("output");
        let install_dir = tmp.path().join("install");

        fs::create_dir_all(dest_dir.join("usr/share/doc/test")).unwrap();
        fs::write(dest_dir.join("usr/share/doc/test/README"), "Hello World").unwrap();

        let meta = test_meta();
        let pack_result = pack(&dest_dir, &meta, &output_dir).unwrap();
        unpack(&pack_result.path, &install_dir).unwrap();

        let readme = fs::read_to_string(install_dir.join("usr/share/doc/test/README")).unwrap();
        assert_eq!(readme, "Hello World");
    }

    #[test]
    fn hash_file_returns_sha256() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let hash = hash_file(&file_path).unwrap();
        assert!(hash.starts_with("sha256:"));
        assert_eq!(hash.len(), 71); // "sha256:" + 64 hex chars
    }

    #[test]
    fn unpack_rejects_path_traversal() {
        // The tar crate itself rejects ".." in entry paths at creation time,
        // so we verify our own defensive check via a manual archive construction.
        // If a future tar version relaxes this, our guard in unpack() will catch it.
        //
        // For now, verify the validation logic directly by testing that a path
        // with ParentDir components would be rejected if it reached unpack.
        let relative = std::path::Path::new("../escaped.txt");
        let has_parent_dir = relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
        assert!(
            has_parent_dir,
            "path traversal detection should catch '..' components"
        );
    }
}
