use crate::domain::DomainError;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Persist bytes using a crash-safe temp-file + fsync + atomic rename sequence.
///
/// The file contents are synced before publication, and the parent directory
/// is synced on Unix so the rename itself is durable across abrupt power loss.
pub fn write_atomic(path: &Path, content: &[u8]) -> Result<(), DomainError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| {
        DomainError::Internal(format!(
            "Failed to create parent directory '{}': {}",
            parent.display(),
            e
        ))
    })?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("atomic");
    let tmp_path = parent.join(format!(".{}.tmp", file_name));

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp_path)
        .map_err(|e| {
            DomainError::Internal(format!(
                "Failed to create atomic temp file '{}': {}",
                tmp_path.display(),
                e
            ))
        })?;

    file.write_all(content).map_err(|e| {
        DomainError::Internal(format!(
            "Failed to write atomic temp file '{}': {}",
            tmp_path.display(),
            e
        ))
    })?;
    file.sync_all().map_err(|e| {
        DomainError::Internal(format!(
            "Failed to fsync atomic temp file '{}': {}",
            tmp_path.display(),
            e
        ))
    })?;
    drop(file);

    fs::rename(&tmp_path, path).map_err(|e| {
        DomainError::Internal(format!(
            "Failed to atomically publish '{}': {}",
            path.display(),
            e
        ))
    })?;

    #[cfg(unix)]
    {
        let dir = File::open(parent).map_err(|e| {
            DomainError::Internal(format!(
                "Failed to open parent directory '{}' for fsync: {}",
                parent.display(),
                e
            ))
        })?;
        dir.sync_all().map_err(|e| {
            DomainError::Internal(format!(
                "Failed to fsync parent directory '{}': {}",
                parent.display(),
                e
            ))
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::write_atomic;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn replaces_target_without_leaving_temp_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("state.json");

        write_atomic(&target, b"first").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"first");

        write_atomic(&target, b"second").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second");
        assert!(!dir.path().join(".state.json.tmp").exists());
    }
}
