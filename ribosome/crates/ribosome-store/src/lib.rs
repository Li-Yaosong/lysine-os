#![deny(clippy::all)]
#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod digest;
pub mod error;
pub mod gc;
pub mod refs;
pub mod store;

pub use digest::Sha256Digest;
pub use error::{Result, StoreError};
pub use gc::GcStats;
pub use store::{ObjectHandle, VacuoleStore};

use sha2::{Digest, Sha256};
use std::path::Path;

/// Compute SHA-256 of a file, returning `sha256:<hex>`.
///
/// This is the canonical hash function used across Ribosome.
/// Previously duplicated in `ribosome-package` and `ribosome-repository`,
/// now unified here.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    let result = hasher.finalize();
    Ok(format!("sha256:{result:x}"))
}
