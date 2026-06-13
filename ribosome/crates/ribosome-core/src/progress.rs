//! Build progress reporting interface.
//!
//! Defines the `BuildProgress` trait for reporting build lifecycle events.
//! CLI consumers implement this trait to display progress (e.g. pip-style text).
//! Library consumers can use `NoProgress` to silence all progress output.

/// Callback interface for build progress events.
///
/// All methods receive only the data needed for display; the implementation
/// decides how to format and output it.
pub trait BuildProgress: Send + Sync {
    /// A package build is starting.
    ///
    /// `index` is 1-based. For single-package builds, `index == 1 && total == 1`.
    fn package_started(&self, index: usize, total: usize, name: &str);

    /// A package build has finished.
    fn package_finished(&self, name: &str, success: bool, duration: std::time::Duration);

    /// A build phase (prepare/compile/check/install) is starting.
    fn phase_started(&self, phase: &str);

    /// A build phase has finished.
    fn phase_finished(&self, phase: &str, success: bool, duration: std::time::Duration);

    /// A single file was extracted from a source tarball.
    fn on_extract_file(&self, count: usize, filename: &str);

    /// Source extraction is complete.
    fn extract_done(&self, total_files: usize);

    /// A line of output from the build subprocess (stdout or stderr).
    fn build_output(&self, line: &str);

    /// A single file was packed into the .prot archive.
    fn on_pack_file(&self, count: usize);

    /// Packing is complete.
    fn pack_done(&self, file_count: usize, size_bytes: u64, filename: &str);
}

/// No-op progress reporter. All callbacks do nothing.
pub struct NoProgress;

impl BuildProgress for NoProgress {
    fn package_started(&self, _index: usize, _total: usize, _name: &str) {}
    fn package_finished(&self, _name: &str, _success: bool, _duration: std::time::Duration) {}
    fn phase_started(&self, _phase: &str) {}
    fn phase_finished(&self, _phase: &str, _success: bool, _duration: std::time::Duration) {}
    fn on_extract_file(&self, _count: usize, _filename: &str) {}
    fn extract_done(&self, _total_files: usize) {}
    fn build_output(&self, _line: &str) {}
    fn on_pack_file(&self, _count: usize) {}
    fn pack_done(&self, _file_count: usize, _size_bytes: u64, _filename: &str) {}
}
