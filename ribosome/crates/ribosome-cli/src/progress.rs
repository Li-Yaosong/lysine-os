//! Pip-style text progress reporter for build operations.
//!
//! Outputs concise, line-by-line progress similar to `pip install`:
//!
//! ```text
//! [1/5] cross-binutils-2.42
//!   Extracting source... 45 files
//!   [prepare] Running...
//!   [compile] Running...
//!     checking build system type... x86_64-lysine-linux-gnu
//!     make -j16
//!   [check]   Skipped
//!   [install] Running...
//!   Packing 128 files -> cross-binutils-2.42-1-x86_64.prot (12.3 MiB)
//!   OK (2m 15s)
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};

use ribosome_core::BuildProgress;

/// Pip-style text progress reporter.
///
/// Thread-safe: uses interior mutability so it can be shared across threads.
pub struct PipStyleProgress {
    /// Count of extracted files (updated by on_extract_file, read by extract_done).
    extract_count: AtomicUsize,
}

impl PipStyleProgress {
    pub fn new() -> Self {
        Self {
            extract_count: AtomicUsize::new(0),
        }
    }

    fn format_duration(duration: std::time::Duration) -> String {
        let secs = duration.as_secs();
        if secs < 60 {
            format!("{secs}s")
        } else {
            let mins = secs / 60;
            let remain_secs = secs % 60;
            format!("{mins}m {remain_secs}s")
        }
    }
}

impl BuildProgress for PipStyleProgress {
    fn package_started(&self, index: usize, total: usize, name: &str) {
        // Reset counter for the new package
        self.extract_count.store(0, Ordering::SeqCst);

        if total > 1 {
            println!("[{index}/{total}] {name}");
        } else {
            println!("{name}");
        }
    }

    fn package_finished(&self, _name: &str, success: bool, duration: std::time::Duration) {
        if success {
            println!("  OK ({})", Self::format_duration(duration));
        } else {
            println!("  FAILED ({})", Self::format_duration(duration));
        }
        // Blank line between packages
        println!();
    }

    fn phase_started(&self, phase: &str) {
        let aligned = format!("{phase:10}");
        println!("  [{aligned}] Running...");
    }

    fn phase_finished(&self, phase: &str, success: bool, _duration: std::time::Duration) {
        if !success {
            let aligned = format!("{phase:10}");
            println!("  [{aligned}] FAILED");
        }
    }

    fn on_extract_file(&self, count: usize, _filename: &str) {
        self.extract_count.store(count, Ordering::SeqCst);
    }

    fn extract_done(&self, _total_files: usize) {
        let count = self.extract_count.load(Ordering::SeqCst);
        if count > 0 {
            println!("  Extracting source... {count} files");
        }
    }

    fn build_output(&self, line: &str) {
        // Only print lines that are not empty to avoid excessive blank lines
        if !line.trim().is_empty() {
            println!("    {line}");
        }
    }

    fn on_pack_file(&self, _count: usize) {}

    fn pack_done(&self, file_count: usize, size_bytes: u64, filename: &str) {
        let size_str = format_size(size_bytes);
        println!("  Packing {file_count} files -> {filename} ({size_str})");
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{} MiB", bytes / (1024 * 1024))
    }
}
