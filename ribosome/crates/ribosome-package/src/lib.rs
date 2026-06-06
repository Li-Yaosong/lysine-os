pub mod error;
pub mod packer;

pub use error::{PackageError, Result};
pub use packer::{hash_file, pack, read_meta, unpack, PackResult, PackageMeta, ProtMeta};
