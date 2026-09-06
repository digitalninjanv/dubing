use crate::application::ports::SecretStore;
use crate::domain::DomainError;
use crate::infrastructure::filesystem::AppPaths;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const CREDENTIALS_FILE: &str = "credentials";
const KEY_NAME: &str = "gemini_api_key";

pub struct StandardSecretStore {
    fallback_path: PathBuf,
}

impl StandardSecretStore {
    pub fn new() -> Self {
        let fallback_path = AppPaths::config_dir().join(CREDENTIALS_FILE);
        Self { fallback_path }
    }
}

impl Default for StandardSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for StandardSecretStore {
    fn get_api_key(&self) -> Result<Option<String>, DomainError> {
        // 1. Check environment variable first
        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            let trimmed = key.trim();
            if !trimmed.is_empty() {
                return Ok(Some(trimmed.to_string()));
            }
        }

        // 2. Check fallback credentials file with secure 0600 permissions
        if self.fallback_path.exists() {
            let content = fs::read_to_string(&self.fallback_path).map_err(|e| {
                DomainError::Internal(format!("Failed to read credentials file: {}", e))
            })?;

            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(val) = trimmed.strip_prefix(&format!("{}=", KEY_NAME)) {
                    let key_val = val.trim();
                    if !key_val.is_empty() {
                        return Ok(Some(key_val.to_string()));
                    }
                }
            }
        }

        Ok(None)
    }

    fn set_api_key(&self, key: &str) -> Result<(), DomainError> {
        let parent = self.fallback_path.parent().unwrap_or(&self.fallback_path);
        let _ = fs::create_dir_all(parent);

        let trimmed = key.trim();
        let content = format!("{}={}\n", KEY_NAME, trimmed);

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.fallback_path)
            .map_err(|e| {
                DomainError::Internal(format!(
                    "Failed to open credentials file for writing: {}",
                    e
                ))
            })?;

        // Secure file permissions on Unix: 0600 (owner read/write only)
        #[cfg(unix)]
        {
            let perms = fs::Permissions::from_mode(0o600);
            let _ = file.set_permissions(perms);
        }

        file.write_all(content.as_bytes())
            .map_err(|e| DomainError::Internal(format!("Failed to write credentials: {}", e)))?;

        Ok(())
    }

    fn delete_api_key(&self) -> Result<(), DomainError> {
        if self.fallback_path.exists() {
            let _ = fs::remove_file(&self.fallback_path);
        }
        Ok(())
    }
}
