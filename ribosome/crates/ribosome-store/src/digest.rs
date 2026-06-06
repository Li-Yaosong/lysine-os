use std::fmt;
use std::path::PathBuf;

use crate::error::{Result, StoreError};

/// A SHA-256 digest, stored as 32 raw bytes.
///
/// Display format is lowercase hex (64 characters).
/// Can be parsed from `"sha256:<hex>"` or bare `"<hex>"`.
#[derive(Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// The digest as raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex representation (64 characters, no prefix).
    #[must_use]
    pub fn hex(&self) -> String {
        format!("{self:x}")
    }

    /// Full string with `sha256:` prefix, matching Ribosome's canonical format.
    #[must_use]
    pub fn to_prefixed(&self) -> String {
        format!("sha256:{self:x}")
    }

    /// Parse from `sha256:<hex>` or bare `<hex>` (64 hex characters).
    ///
    /// # Errors
    ///
    /// Returns `StoreError::InvalidDigest` if the input is not valid hex or wrong length.
    pub fn parse(s: &str) -> Result<Self> {
        let hex = s.strip_prefix("sha256:").unwrap_or(s);
        if hex.len() != 64 {
            return Err(StoreError::InvalidDigest(format!(
                "expected 64 hex characters, got {}",
                hex.len()
            )));
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|e| {
                StoreError::InvalidDigest(format!("invalid hex at position {}: {e}", i * 2))
            })?;
        }
        Ok(Self(bytes))
    }

    /// Compute SHA-256 digest from raw bytes.
    #[must_use]
    pub fn from_bytes(data: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        Self(result.into())
    }

    /// Construct from raw 32-byte array (no hashing).
    pub(crate) fn from_bytes_raw(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Directory shard: first 2 hex characters (256 possible shards).
    #[must_use]
    pub fn shard(&self) -> String {
        format!("{:02x}", self.0[0])
    }

    /// File name within a shard: remaining 62 hex characters.
    #[must_use]
    pub fn file_name(&self) -> String {
        self.0
            .iter()
            .skip(1)
            .fold(String::with_capacity(62), |mut acc, b| {
                use std::fmt::Write;
                let _ = write!(acc, "{b:02x}");
                acc
            })
    }

    /// Full blob path under `objects/`: `objects/<shard>/<file_name>`.
    #[must_use]
    pub fn object_path(&self, root: &std::path::Path) -> PathBuf {
        root.join("objects")
            .join(self.shard())
            .join(self.file_name())
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256Digest({self:x})")
    }
}

impl fmt::LowerHex for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:x}")
    }
}

impl std::str::FromStr for Sha256Digest {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_display_parse() {
        let digest = Sha256Digest::from_bytes(b"hello world");
        let displayed = digest.to_string();
        let parsed: Sha256Digest = displayed.parse().unwrap();
        assert_eq!(digest, parsed);
    }

    #[test]
    fn roundtrip_prefixed_parse() {
        let digest = Sha256Digest::from_bytes(b"test data");
        let prefixed = digest.to_prefixed();
        assert!(prefixed.starts_with("sha256:"));
        let parsed = Sha256Digest::parse(&prefixed).unwrap();
        assert_eq!(digest, parsed);
    }

    #[test]
    fn bare_hex_parse() {
        let digest = Sha256Digest::from_bytes(b"test");
        let hex = digest.hex();
        assert_eq!(hex.len(), 64);
        let parsed = Sha256Digest::parse(&hex).unwrap();
        assert_eq!(digest, parsed);
    }

    #[test]
    fn reject_invalid_length() {
        let err = Sha256Digest::parse("abc123");
        assert!(err.is_err());
        assert!(matches!(err.unwrap_err(), StoreError::InvalidDigest(_)));
    }

    #[test]
    fn reject_invalid_hex() {
        let err = Sha256Digest::parse(&"z".repeat(64));
        assert!(err.is_err());
    }

    #[test]
    fn known_hash_value() {
        // SHA-256("hello world") known value
        let digest = Sha256Digest::from_bytes(b"hello world");
        assert_eq!(
            digest.hex(),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn shard_is_first_two_hex_chars() {
        let digest = Sha256Digest::from_bytes(b"hello world");
        assert_eq!(digest.shard(), "b9");
    }

    #[test]
    fn object_path_structure() {
        let digest = Sha256Digest::from_bytes(b"hello world");
        let root = std::path::Path::new("/var/ribosome/vacuole");
        let path = digest.object_path(root);
        assert_eq!(
            path,
            std::path::PathBuf::from("/var/ribosome/vacuole/objects/b9/4d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9")
        );
    }

    #[test]
    fn ordering_consistency() {
        let a = Sha256Digest::from_bytes(b"a");
        let b = Sha256Digest::from_bytes(b"b");
        // Ordering is deterministic (may be a < b or b < a, but must be consistent)
        assert_eq!(a < b, !(b < a) || a == b);
    }
}
