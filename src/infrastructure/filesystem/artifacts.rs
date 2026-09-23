use crate::domain::DomainError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

pub const ARTIFACT_MANIFEST_SCHEMA_VERSION: u32 = 1;
const MANIFEST_FILE_NAME: &str = "artifacts.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub key: String,
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub provenance_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub schema_version: u32,
    pub artifacts: BTreeMap<String, ArtifactRecord>,
}

impl Default for ArtifactManifest {
    fn default() -> Self {
        Self {
            schema_version: ARTIFACT_MANIFEST_SCHEMA_VERSION,
            artifacts: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    job_dir: PathBuf,
}

impl ArtifactStore {
    pub fn new(job_dir: impl Into<PathBuf>) -> Self {
        Self {
            job_dir: job_dir.into(),
        }
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.job_dir.join(MANIFEST_FILE_NAME)
    }

    pub fn load(&self) -> Result<ArtifactManifest, DomainError> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(ArtifactManifest::default());
        }

        let content = std::fs::read_to_string(&path).map_err(|e| {
            DomainError::Internal(format!("Failed to read artifact manifest: {}", e))
        })?;

        let manifest: ArtifactManifest = serde_json::from_str(&content).map_err(|e| {
            DomainError::Internal(format!("Failed to parse artifact manifest: {}", e))
        })?;

        if manifest.schema_version != ARTIFACT_MANIFEST_SCHEMA_VERSION {
            return Err(DomainError::Configuration(format!(
                "Unsupported artifact manifest schema version {} (expected {})",
                manifest.schema_version, ARTIFACT_MANIFEST_SCHEMA_VERSION
            )));
        }

        Ok(manifest)
    }

    pub fn register(
        &self,
        key: impl Into<String>,
        path: &Path,
        manifest: &mut ArtifactManifest,
    ) -> Result<ArtifactRecord, DomainError> {
        self.register_with_provenance(key, path, manifest, None)
    }

    pub fn register_with_provenance(
        &self,
        key: impl Into<String>,
        path: &Path,
        manifest: &mut ArtifactManifest,
        provenance_sha256: Option<String>,
    ) -> Result<ArtifactRecord, DomainError> {
        let key = key.into();
        let relative_path = path.strip_prefix(&self.job_dir).map_err(|_| {
            DomainError::Internal(format!(
                "Artifact path '{}' is outside job directory '{}'",
                path.display(),
                self.job_dir.display()
            ))
        })?;

        let record = Self::record_path(
            key.clone(),
            relative_path.to_path_buf(),
            path,
            provenance_sha256,
        )?;
        manifest.artifacts.insert(key, record.clone());
        Ok(record)
    }

    pub fn verify(
        &self,
        key: &str,
        manifest: &ArtifactManifest,
    ) -> Result<Option<PathBuf>, DomainError> {
        self.verify_with_provenance(key, manifest, None)
    }

    pub fn verify_with_provenance(
        &self,
        key: &str,
        manifest: &ArtifactManifest,
        expected_provenance_sha256: Option<&str>,
    ) -> Result<Option<PathBuf>, DomainError> {
        let Some(record) = manifest.artifacts.get(key) else {
            return Ok(None);
        };

        if let Some(expected) = expected_provenance_sha256 {
            if record.provenance_sha256.as_deref() != Some(expected) {
                tracing::debug!(
                    "Artifact '{}' provenance mismatch; regenerating dependent output",
                    key
                );
                return Ok(None);
            }
        }

        let path = self.job_dir.join(&record.relative_path);
        if !path.is_file() {
            return Ok(None);
        }

        let metadata = std::fs::metadata(&path).map_err(|e| {
            DomainError::Internal(format!(
                "Failed to stat artifact '{}': {}",
                path.display(),
                e
            ))
        })?;

        if metadata.len() != record.size_bytes {
            tracing::warn!(
                "Artifact '{}' size mismatch: manifest={}, actual={}",
                key,
                record.size_bytes,
                metadata.len()
            );
            return Ok(None);
        }

        let actual_hash = sha256_file(&path)?;
        if actual_hash != record.sha256 {
            tracing::warn!(
                "Artifact '{}' checksum mismatch; invalidating cached output",
                key
            );
            return Ok(None);
        }

        Ok(Some(path))
    }

    pub fn invalidate_prefix(manifest: &mut ArtifactManifest, prefix: &str) {
        manifest.artifacts.retain(|key, _| !key.starts_with(prefix));
    }

    pub fn save(&self, manifest: &ArtifactManifest) -> Result<(), DomainError> {
        let content = serde_json::to_string_pretty(manifest).map_err(|e| {
            DomainError::Internal(format!("Failed to serialize artifact manifest: {}", e))
        })?;
        write_atomic(&self.manifest_path(), &content)
    }

    fn record_path(
        key: String,
        relative_path: PathBuf,
        path: &Path,
        provenance_sha256: Option<String>,
    ) -> Result<ArtifactRecord, DomainError> {
        let metadata = std::fs::metadata(path).map_err(|e| {
            DomainError::Internal(format!(
                "Failed to stat artifact '{}': {}",
                path.display(),
                e
            ))
        })?;

        if !metadata.is_file() || metadata.len() == 0 {
            return Err(DomainError::Internal(format!(
                "Artifact '{}' is missing or empty",
                path.display()
            )));
        }

        Ok(ArtifactRecord {
            key,
            relative_path,
            size_bytes: metadata.len(),
            sha256: sha256_file(path)?,
            provenance_sha256,
        })
    }

    #[cfg(test)]
    pub fn from_manifest(job_dir: impl Into<PathBuf>, manifest: ArtifactManifest) -> Self {
        let store = Self::new(job_dir);
        let _ = write_atomic(
            &store.manifest_path(),
            &serde_json::to_string_pretty(&manifest).unwrap(),
        );
        store
    }
}

fn sha256_file(path: &Path) -> Result<String, DomainError> {
    let file = File::open(path).map_err(|e| {
        DomainError::Internal(format!(
            "Failed to open artifact '{}' for hashing: {}",
            path.display(),
            e
        ))
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];

    loop {
        let read = reader.read(&mut buffer).map_err(|e| {
            DomainError::Internal(format!(
                "Failed to read artifact '{}' for hashing: {}",
                path.display(),
                e
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn write_atomic(path: &Path, content: &str) -> Result<(), DomainError> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content).map_err(|e| {
        DomainError::Internal(format!(
            "Failed to write artifact manifest temp file: {}",
            e
        ))
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        DomainError::Internal(format!("Failed to publish artifact manifest: {}", e))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ArtifactManifest, ArtifactStore};
    use tempfile::tempdir;

    #[test]
    fn register_and_verify_artifact() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("artifact.json");
        std::fs::write(&path, br#"{"ok":true}"#).unwrap();

        let store = ArtifactStore::new(dir.path());
        let mut manifest = ArtifactManifest::default();
        store.register("transcript", &path, &mut manifest).unwrap();

        assert_eq!(
            store.verify("transcript", &manifest).unwrap(),
            Some(path.clone())
        );
    }

    #[test]
    fn provenance_mismatch_invalidates_artifact() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("artifact.bin");
        std::fs::write(&path, b"cached").unwrap();

        let store = ArtifactStore::new(dir.path());
        let mut manifest = ArtifactManifest::default();
        store
            .register_with_provenance(
                "tts",
                &path,
                &mut manifest,
                Some("provenance-a".to_string()),
            )
            .unwrap();

        assert_eq!(
            store
                .verify_with_provenance("tts", &manifest, Some("provenance-b"))
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .verify_with_provenance("tts", &manifest, Some("provenance-a"))
                .unwrap(),
            Some(path)
        );
    }

    #[test]
    fn legacy_artifact_without_provenance_is_not_reused_when_expected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("artifact.bin");
        std::fs::write(&path, b"legacy").unwrap();

        let store = ArtifactStore::new(dir.path());
        let mut manifest = ArtifactManifest::default();
        store.register("tts", &path, &mut manifest).unwrap();

        assert_eq!(
            store
                .verify_with_provenance("tts", &manifest, Some("new"))
                .unwrap(),
            None
        );
    }

    #[test]
    fn invalidate_prefix_removes_dependent_artifacts_only() {
        let mut manifest = ArtifactManifest::default();
        manifest.artifacts.insert(
            "synthesis/0000".to_string(),
            super::ArtifactRecord {
                key: "synthesis/0000".to_string(),
                relative_path: "a.wav".into(),
                size_bytes: 1,
                sha256: "a".to_string(),
                provenance_sha256: Some("p".to_string()),
            },
        );
        manifest.artifacts.insert(
            "translation".to_string(),
            super::ArtifactRecord {
                key: "translation".to_string(),
                relative_path: "translation.json".into(),
                size_bytes: 1,
                sha256: "b".to_string(),
                provenance_sha256: None,
            },
        );

        ArtifactStore::invalidate_prefix(&mut manifest, "synthesis/");
        assert!(!manifest.artifacts.contains_key("synthesis/0000"));
        assert!(manifest.artifacts.contains_key("translation"));
    }

    #[test]
    fn checksum_mismatch_invalidates_artifact() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("artifact.json");
        std::fs::write(&path, b"original").unwrap();

        let store = ArtifactStore::new(dir.path());
        let mut manifest = ArtifactManifest::default();
        store.register("transcript", &path, &mut manifest).unwrap();

        std::fs::write(&path, b"tampered").unwrap();

        assert_eq!(store.verify("transcript", &manifest).unwrap(), None);
    }

    #[test]
    fn missing_manifest_is_safe_to_start_from_scratch() {
        let dir = tempdir().unwrap();
        let store = ArtifactStore::new(dir.path());
        assert_eq!(store.load().unwrap(), ArtifactManifest::default());
    }

    #[test]
    fn manifest_persists_and_reloads() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("artifact.bin");
        std::fs::write(&path, b"persist-me").unwrap();

        let store = ArtifactStore::new(dir.path());
        let mut manifest = ArtifactManifest::default();
        store.register("artifact", &path, &mut manifest).unwrap();
        store.save(&manifest).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded, manifest);
        assert_eq!(store.verify("artifact", &loaded).unwrap(), Some(path));
    }
}
