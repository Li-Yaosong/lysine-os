//! Source tarball fetcher and extractor for Ribosome.
//!
//! Downloads source tarballs declared in mRNA `sources` fields, verifies
//! their SHA-256 hashes, stores them in the vacuole CAS, and extracts
//! them to SRCDIR before build phases execute.

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use ribosome_parser::MrnaFile;
use ribosome_store::VacuoleStore;

use crate::error::{CoreError, Result};

// ---------------------------------------------------------------------------
// Fetch types and batch reporting
// ---------------------------------------------------------------------------

/// Report returned after fetching sources for a single package.
#[derive(Debug)]
pub struct FetchReport {
    /// Package name.
    pub package: String,
    /// Number of sources successfully fetched (downloaded or already cached).
    pub fetched: usize,
    /// Number of sources skipped (e.g. signature-only files without a hash).
    pub skipped: usize,
    /// Number of sources that failed.
    pub failed: usize,
    /// Details of each failure.
    pub errors: Vec<FetchError>,
}

/// A single source fetch failure.
#[derive(Debug)]
pub struct FetchError {
    pub url: String,
    pub reason: String,
}

/// Aggregate report for fetching multiple packages.
#[derive(Debug, Default)]
pub struct BatchFetchReport {
    pub packages_processed: usize,
    pub sources_fetched: usize,
    pub sources_skipped: usize,
    pub sources_failed: usize,
    pub errors: Vec<(String, FetchError)>,
}

// ---------------------------------------------------------------------------
// Fetch: download sources and store in CAS
// ---------------------------------------------------------------------------

/// Fetch all sources declared in an mRNA file, storing them in the vacuole CAS.
///
/// For each source entry:
/// 1. Extract the filename from the URL
/// 2. Check if a source ref already exists in CAS
/// 3. If not, download to a temp file, verify hash, store in CAS
///
/// Source files without a `hash` field are skipped with a warning
/// (signature-only files like `.sig` or `.asc`).
pub fn fetch_sources(mrna: &MrnaFile, store: &VacuoleStore) -> Result<FetchReport> {
    let mut report = FetchReport {
        package: mrna.name.clone(),
        fetched: 0,
        skipped: 0,
        failed: 0,
        errors: Vec::new(),
    };

    let mirrors = load_mirrors();

    for source in &mrna.sources {
        let filename = url_filename(&source.url);

        // Check if already cached
        if store
            .resolve_source_ref(&filename)
            .unwrap_or(None)
            .is_some()
        {
            info!(package = %mrna.name, file = %filename, "source already cached, skipping");
            report.fetched += 1;
            continue;
        }

        // Skip sources without a hash (signature files)
        let expected_hash = match &source.hash {
            Some(h) => h.clone(),
            None => {
                warn!(package = %mrna.name, url = %source.url, "skipping source without hash");
                report.skipped += 1;
                continue;
            }
        };

        info!(package = %mrna.name, url = %source.url, "downloading source");

        // Build candidate URL list: mirror replacements first, then original.
        let mut urls: Vec<String> = Vec::new();
        for (from, to) in &mirrors {
            if source.url.starts_with(from) {
                let replaced = format!("{}{}", to, &source.url[from.len()..]);
                urls.push(replaced);
            }
        }
        urls.push(source.url.clone());

        let mut last_error: Option<DownloadError> = None;
        let mut success = false;

        for url in &urls {
            if url != &source.url {
                info!(package = %mrna.name, mirror = %url, "trying mirror");
            }
            match download_and_verify(url, &expected_hash) {
                Ok(data) => {
                    let digest = store.put_bytes(&data).map_err(|e| {
                        CoreError::io(PathBuf::from(&filename), format!("CAS store failed: {e}"))
                    })?;
                    store.add_source_ref(&filename, &digest).map_err(|e| {
                        CoreError::io(PathBuf::from(&filename), format!("CAS ref failed: {e}"))
                    })?;
                    info!(
                        package = %mrna.name, file = %filename,
                        hash = %digest.to_prefixed(), size = data.len(),
                        "source fetched and cached"
                    );
                    report.fetched += 1;
                    success = true;
                    break;
                }
                Err(e) => {
                    if url == &source.url {
                        warn!(package = %mrna.name, url = %url, error = %e, "source download failed");
                    } else {
                        warn!(package = %mrna.name, mirror = %url, error = %e, "mirror download failed, falling back");
                    }
                    last_error = Some(e);
                }
            }
        }

        if !success {
            report.failed += 1;
            report.errors.push(FetchError {
                url: source.url.clone(),
                reason: last_error.map(|e| e.to_string()).unwrap_or_default(),
            });
        }
    }

    Ok(report)
}

/// Fetch sources for multiple mRNA files, collecting an aggregate report.
pub fn fetch_sources_batch(mrnas: &[MrnaFile], store: &VacuoleStore) -> BatchFetchReport {
    let mut batch = BatchFetchReport::default();

    for mrna in mrnas {
        batch.packages_processed += 1;
        match fetch_sources(mrna, store) {
            Ok(report) => {
                batch.sources_fetched += report.fetched;
                batch.sources_skipped += report.skipped;
                batch.sources_failed += report.failed;
                for err in report.errors {
                    batch.errors.push((mrna.name.clone(), err));
                }
            }
            Err(e) => {
                batch.sources_failed += mrna.sources.len();
                batch.errors.push((
                    mrna.name.clone(),
                    FetchError {
                        url: String::new(),
                        reason: e.to_string(),
                    },
                ));
            }
        }
    }

    batch
}

// ---------------------------------------------------------------------------
// Extract: retrieve from CAS and extract to SRCDIR
// ---------------------------------------------------------------------------

/// Extract a source tarball from the CAS to the SRCDIR.
///
/// The first source in the mRNA's `sources` list with a hash is used.
/// If the archive contains a single top-level directory, its contents
/// are hoisted up to `src_dir` directly (e.g. `gcc-14.2.0/` -> `src_dir/`).
///
/// If no source is found in CAS, this is a no-op.
pub fn extract_source(mrna: &MrnaFile, store: &VacuoleStore, src_dir: &Path) -> Result<()> {
    if mrna.sources.is_empty() {
        debug!(package = %mrna.name, "no sources declared, skipping extraction");
        return Ok(());
    }

    let source = match mrna.sources.iter().find(|s| s.hash.is_some()) {
        Some(s) => s,
        None => {
            debug!(package = %mrna.name, "no source with hash, skipping extraction");
            return Ok(());
        }
    };

    let filename = url_filename(&source.url);

    let digest = store
        .resolve_source_ref(&filename)
        .map_err(|e| CoreError::io(&filename, format!("resolve source ref: {e}")))?;

    let digest = match digest {
        Some(d) => d,
        None => {
            return Err(CoreError::BuildFailed {
                package: mrna.name.clone(),
                reason: format!("source '{filename}' not in CAS — run 'ribosome fetch' first"),
            });
        }
    };

    let handle = store
        .get(&digest)
        .map_err(|e| CoreError::io(&filename, format!("CAS get: {e}")))?;

    let handle = match handle {
        Some(h) => h,
        None => {
            warn!(package = %mrna.name, file = %filename, "source object missing from CAS");
            return Ok(());
        }
    };

    let cas_path = handle.path().to_path_buf();
    info!(package = %mrna.name, file = %filename, "extracting source to {}", src_dir.display());

    std::fs::create_dir_all(src_dir).map_err(|e| CoreError::io(src_dir, e.to_string()))?;

    extract_tarball(&cas_path, src_dir, &filename)?;

    Ok(())
}

/// Resolve a source tarball from CAS by mRNA source entry, returning the
/// on-disk path to the CAS object.
pub fn resolve_source(
    mrna: &MrnaFile,
    source_index: usize,
    store: &VacuoleStore,
) -> Result<Option<PathBuf>> {
    let source = mrna.sources.get(source_index).ok_or_else(|| {
        CoreError::InvalidConfig(format!(
            "source index {source_index} out of range for {} (has {} sources)",
            mrna.name,
            mrna.sources.len()
        ))
    })?;

    let filename = url_filename(&source.url);
    let digest = store
        .resolve_source_ref(&filename)
        .map_err(|e| CoreError::io(&filename, format!("resolve source ref: {e}")))?;

    match digest {
        Some(d) => {
            let handle = store
                .get(&d)
                .map_err(|e| CoreError::io(&filename, format!("CAS get: {e}")))?;
            Ok(handle.map(|h| h.path().to_path_buf()))
        }
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Maximum number of download retries per URL.
const MAX_DOWNLOAD_RETRIES: u32 = 3;

/// Download a file from URL and verify its SHA-256 hash.
///
/// Retries up to `MAX_DOWNLOAD_RETRIES` times on transient network errors
/// (connection reset, timeout, partial body read) before giving up.
fn download_and_verify(
    url: &str,
    expected_hash: &str,
) -> std::result::Result<Vec<u8>, DownloadError> {
    let expected_hex = expected_hash
        .strip_prefix("sha256:")
        .ok_or_else(|| DownloadError {
            url: url.to_string(),
            reason: format!("hash must start with 'sha256:' prefix, got: {expected_hash}"),
        })?;
    if expected_hex.len() != 64 {
        return Err(DownloadError {
            url: url.to_string(),
            reason: format!("hash must be 64 hex characters, got {}", expected_hex.len()),
        });
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .user_agent("ribosome/0.1.0 (LysineOS build system)")
        .build()
        .map_err(|e| DownloadError {
            url: url.to_string(),
            reason: format!("HTTP client creation failed: {e}"),
        })?;

    let mut last_err: Option<DownloadError> = None;

    for attempt in 0..=MAX_DOWNLOAD_RETRIES {
        if attempt > 0 {
            let delay = std::time::Duration::from_secs(2u64.pow(attempt - 1));
            warn!(url = %url, attempt, "retrying download after {:?}", delay);
            std::thread::sleep(delay);
        }

        match try_download(&client, url, expected_hex) {
            Ok(data) => return Ok(data),
            Err(e) => {
                let is_transient = e.reason.contains("error decoding response body")
                    || e.reason.contains("error sending request")
                    || e.reason.contains("timed out")
                    || e.reason.contains("connection reset")
                    || e.reason.contains("broken pipe");

                if is_transient && attempt < MAX_DOWNLOAD_RETRIES {
                    warn!(url = %url, attempt, error = %e.reason, "download failed, will retry");
                    last_err = Some(e);
                    continue;
                }
                return Err(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| DownloadError {
        url: url.to_string(),
        reason: "all retries exhausted".to_string(),
    }))
}

/// Single download attempt: GET url, read body, verify hash.
fn try_download(
    client: &reqwest::blocking::Client,
    url: &str,
    expected_hex: &str,
) -> std::result::Result<Vec<u8>, DownloadError> {
    let response = client.get(url).send().map_err(|e| DownloadError {
        url: url.to_string(),
        reason: format!("HTTP request failed: {e}"),
    })?;

    if !response.status().is_success() {
        return Err(DownloadError {
            url: url.to_string(),
            reason: format!("HTTP {}", response.status()),
        });
    }

    let data = response.bytes().map_err(|e| DownloadError {
        url: url.to_string(),
        reason: format!("failed to read response body: {e}"),
    })?;

    let mut hasher = Sha256::new();
    hasher.update(&data);
    let computed_hex = format!("{:x}", hasher.finalize());

    if computed_hex != expected_hex {
        return Err(DownloadError {
            url: url.to_string(),
            reason: format!("hash mismatch: expected {expected_hex}, computed {computed_hex}"),
        });
    }

    Ok(data.to_vec())
}

/// Extract the filename from a URL path.
fn url_filename(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut s| s.next_back().map(|s| s.to_string()))
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            url.rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or("unknown")
                .to_string()
        })
}

/// Extract a tarball (auto-detecting compression) to a target directory.
///
/// Supports: .tar.gz, .tar.xz, .tar.zst, .tar.bz2, .tar
/// Hoists single top-level directory contents up to `target_dir`.
fn extract_tarball(archive_path: &Path, target_dir: &Path, filename: &str) -> Result<()> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| CoreError::io(archive_path, e.to_string()))?;

    let filename_lower = filename.to_lowercase();

    if filename_lower.ends_with(".tar.gz") || filename_lower.ends_with(".tgz") {
        let decoder = flate2::read::GzDecoder::new(file);
        do_extract(&mut tar::Archive::new(decoder), target_dir)
    } else if filename_lower.ends_with(".tar.xz") || filename_lower.ends_with(".tar.lzma") {
        let decoder = xz2::read::XzDecoder::new(file);
        do_extract(&mut tar::Archive::new(decoder), target_dir)
    } else if filename_lower.ends_with(".tar.zst") {
        let decoder = zstd::Decoder::new(file)
            .map_err(|e| CoreError::io(archive_path, format!("zstd decode: {e}")))?;
        do_extract(&mut tar::Archive::new(decoder), target_dir)
    } else if filename_lower.ends_with(".tar.bz2") {
        let decoder = bzip2::read::BzDecoder::new(file);
        do_extract(&mut tar::Archive::new(decoder), target_dir)
    } else if filename_lower.ends_with(".tar") {
        do_extract(&mut tar::Archive::new(file), target_dir)
    } else {
        Err(CoreError::BuildFailed {
            package: filename.to_string(),
            reason: format!("unsupported archive format: {filename}"),
        })
    }
}

/// Core extraction logic: unpack all entries and optionally hoist single top-level dir.
fn do_extract<R: std::io::Read>(archive: &mut tar::Archive<R>, target_dir: &Path) -> Result<()> {
    let mut top_dirs = std::collections::HashSet::new();

    for entry in archive
        .entries()
        .map_err(|e| CoreError::io(target_dir, format!("tar entries read: {e}")))?
    {
        let mut entry =
            entry.map_err(|e| CoreError::io(target_dir, format!("tar entry read: {e}")))?;

        let path = entry
            .path()
            .map_err(|e| CoreError::io(target_dir, format!("tar entry path: {e}")))?
            .to_path_buf();

        // Reject path traversal
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(CoreError::BuildFailed {
                package: target_dir.to_string_lossy().to_string(),
                reason: format!("tar entry escapes root: {}", path.display()),
            });
        }

        // Track top-level directory name
        if let Some(std::path::Component::Normal(name)) = path.components().next() {
            top_dirs.insert(name.to_string_lossy().into_owned());
        }

        entry
            .unpack_in(target_dir)
            .map_err(|e| CoreError::io(target_dir, format!("extract {}: {e}", path.display())))?;
    }

    // Hoist: if all entries share a single top-level directory
    if top_dirs.len() == 1 {
        let top_name = top_dirs.into_iter().next().unwrap();
        let inner_dir = target_dir.join(&top_name);
        if inner_dir.is_dir() {
            info!(top_dir = %top_name, "hoisting single top-level directory");
            for entry in std::fs::read_dir(&inner_dir)
                .map_err(|e| CoreError::io(&inner_dir, e.to_string()))?
            {
                let entry = entry.map_err(|e| CoreError::io(&inner_dir, e.to_string()))?;
                let dest = target_dir.join(entry.file_name());
                std::fs::rename(entry.path(), &dest)
                    .map_err(|e| CoreError::io(&dest, e.to_string()))?;
            }
            std::fs::remove_dir(&inner_dir)
                .map_err(|e| CoreError::io(&inner_dir, e.to_string()))?;
        }
    }

    Ok(())
}

/// Download error with URL context.
#[derive(Debug)]
pub struct DownloadError {
    pub url: String,
    pub reason: String,
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "download {} failed: {}", self.url, self.reason)
    }
}

impl std::error::Error for DownloadError {}

/// Write source bytes to a temp file (used during build preparation).
pub fn write_source_to_file(data: &[u8], dest: &Path) -> Result<PathBuf> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e.to_string()))?;
    }

    let tmp_path = dest.with_extension("tmp.download");
    let mut f =
        std::fs::File::create(&tmp_path).map_err(|e| CoreError::io(&tmp_path, e.to_string()))?;
    f.write_all(data)
        .map_err(|e| CoreError::io(&tmp_path, e.to_string()))?;
    f.sync_all()
        .map_err(|e| CoreError::io(&tmp_path, e.to_string()))?;
    drop(f);

    std::fs::rename(&tmp_path, dest).map_err(|e| CoreError::io(dest, e.to_string()))?;

    Ok(dest.to_path_buf())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
// Mirror configuration
// ---------------------------------------------------------------------------

/// Load mirror URL replacement rules from environment variables.
///
/// Supports two formats:
/// - `RIBOSOME_MIRROR_N` (N=0,1,2...): value is `old_prefix=new_prefix`
/// - `RIBOSOME_MIRROR_FILE`: path to a file with one `old_prefix=new_prefix` per line
///
/// Example:
///   RIBOSOME_MIRROR_0=https://ftp.gnu.org/gnu/=https://mirrors.ustc.edu.cn/gnu/
///   RIBOSOME_MIRROR_1=https://sourceware.org/pub/=https://mirrors.ustc.edu.cn/sourceware/
fn load_mirrors() -> Vec<(String, String)> {
    let mut mirrors: Vec<(String, String)> = Vec::new();

    // Load from RIBOSOME_MIRROR_0, RIBOSOME_MIRROR_1, ...
    for i in 0..64 {
        if let Ok(val) = std::env::var(format!("RIBOSOME_MIRROR_{i}")) {
            if let Some((from, to)) = val.split_once('=') {
                let from = from.trim().to_string();
                let to = to.trim().to_string();
                if !from.is_empty() && !to.is_empty() {
                    mirrors.push((from, to));
                }
            }
        }
    }

    // Load from RIBOSOME_MIRROR_FILE
    if let Ok(path) = std::env::var("RIBOSOME_MIRROR_FILE") {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((from, to)) = line.split_once('=') {
                    let from = from.trim().to_string();
                    let to = to.trim().to_string();
                    if !from.is_empty() && !to.is_empty() {
                        mirrors.push((from, to));
                    }
                }
            }
        }
    }

    mirrors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_filename_extracts_last_segment() {
        assert_eq!(
            url_filename("https://ftp.gnu.org/gnu/gcc/gcc-14.2.0/gcc-14.2.0.tar.xz"),
            "gcc-14.2.0.tar.xz"
        );
    }

    #[test]
    fn url_filename_handles_simple_url() {
        assert_eq!(
            url_filename("https://example.com/foo-1.0.tar.gz"),
            "foo-1.0.tar.gz"
        );
    }

    #[test]
    fn url_filename_fallback_for_malformed() {
        assert_eq!(url_filename("not-a-url"), "not-a-url");
    }

    #[test]
    fn url_filename_handles_trailing_slash() {
        let result = url_filename("https://example.com/path/");
        assert_eq!(result, "path");
    }

    #[test]
    fn fetch_report_counts_skipped_no_hash() {
        let yaml = r#"
api-version: 1
name: test-pkg
version: 1.0.0
release: 1
description: Test
license: MIT
sources:
  - url: https://example.com/test-1.0.0.tar.xz
    hash: sha256:aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd
  - url: https://example.com/test-1.0.0.tar.xz.sig
    signature: gpg
build:
  install: |
    echo "install"
"#;
        let mrna = ribosome_parser::parse_mrna(yaml).expect("valid mRNA");
        assert_eq!(mrna.sources.len(), 2);

        let tmp = tempfile::tempdir().unwrap();
        let store = VacuoleStore::open(tmp.path()).unwrap();

        let report = fetch_sources(&mrna, &store).expect("fetch should not error");
        assert_eq!(report.skipped, 1, "source without hash should be skipped");
    }

    #[test]
    fn batch_fetch_report_aggregates() {
        let batch = BatchFetchReport {
            packages_processed: 3,
            sources_fetched: 5,
            sources_skipped: 2,
            sources_failed: 1,
            errors: vec![],
        };
        assert_eq!(batch.packages_processed, 3);
        assert_eq!(batch.sources_fetched, 5);
    }

    #[test]
    fn write_source_to_file_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("deep/nested/dir/test.tar.xz");
        let data = b"fake tarball content";

        write_source_to_file(data, &dest).expect("write should succeed");
        assert!(dest.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[test]
    fn extract_tarball_gz_with_single_toplevel() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("test-1.0.tar.gz");
        let extract_dir = tmp.path().join("src");

        // Create a tar.gz with single top-level directory "test-1.0/"
        let file = std::fs::File::create(&tarball_path).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);

        let data = b"hello world";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "test-1.0/README.txt", &data[..])
            .unwrap();

        let data2 = b"#!/bin/sh\necho hi\n";
        let mut header2 = tar::Header::new_gnu();
        header2.set_size(data2.len() as u64);
        header2.set_mode(0o755);
        header2.set_cksum();
        builder
            .append_data(&mut header2, "test-1.0/bin/hello.sh", &data2[..])
            .unwrap();

        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();

        // Extract
        std::fs::create_dir_all(&extract_dir).unwrap();
        extract_tarball(&tarball_path, &extract_dir, "test-1.0.tar.gz").unwrap();

        // After hoisting, files should be directly in extract_dir
        assert!(extract_dir.join("README.txt").exists());
        assert!(extract_dir.join("bin/hello.sh").exists());
        assert!(
            !extract_dir.join("test-1.0").exists(),
            "top-level dir should be hoisted"
        );

        let content = std::fs::read_to_string(extract_dir.join("README.txt")).unwrap();
        assert_eq!(content, "hello world");
    }
}
