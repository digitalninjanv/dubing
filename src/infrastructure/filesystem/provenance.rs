use serde::Serialize;
use sha2::{Digest, Sha256};

/// Bump this whenever the provenance algorithm or semantics change.
pub const PROVENANCE_SCHEMA_VERSION: u32 = 1;

/// Computes a deterministic SHA-256 fingerprint for non-secret stage inputs.
///
/// serde_json object maps are deterministically ordered in the default
/// configuration, so the same semantic provenance produces the same hash.
pub fn fingerprint<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}
