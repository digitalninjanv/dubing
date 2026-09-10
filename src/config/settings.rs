use crate::domain::DomainError;
use crate::infrastructure::filesystem::AppPaths;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    pub transcriber: String,
    pub translator: String,
    pub tts: String,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            transcriber: "gemini-3.5-transcribe".to_string(),
            translator: "gemini-3.1-flash-lite".to_string(),
            tts: "gemini-3.1-flash-tts-preview".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    #[serde(default = "default_max_file_size")]
    pub max_file_size_bytes: u64,
    #[serde(default = "default_bitrate")]
    pub default_bitrate_kbps: u32,
    #[serde(default)]
    pub export_wav: bool,
}

fn default_max_file_size() -> u64 {
    500 * 1024 * 1024
}

fn default_bitrate() -> u32 {
    192
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            max_file_size_bytes: 500 * 1024 * 1024, // 500 MB
            default_bitrate_kbps: 192,
            export_wav: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub models: ModelsConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default = "default_true")]
    pub auto_cleanup: bool,
    #[serde(default)]
    pub debug_mode: bool,
    /// Delete terminal job dirs older than this many days on startup (0 = keep).
    #[serde(default = "default_retention_days")]
    pub job_retention_days: u64,
}

fn default_true() -> bool {
    true
}

fn default_retention_days() -> u64 {
    30
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            models: ModelsConfig::default(),
            audio: AudioConfig::default(),
            auto_cleanup: true,
            debug_mode: false,
            job_retention_days: default_retention_days(),
        }
    }
}

impl AppSettings {
    /// Load from `~/.config/audiodub/config.toml`; missing/corrupt file
    /// falls back to defaults (never fails startup).
    pub fn load() -> Self {
        let path = AppPaths::config_file();
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<AppSettings>(&content) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("Ignoring corrupt settings file {}: {}", path.display(), e);
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// Persist atomically (tmp + rename + fsync).
    pub fn save(&self) -> Result<(), DomainError> {
        let path = AppPaths::config_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DomainError::Internal(format!("Failed to create config dir: {}", e))
            })?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| DomainError::Internal(format!("Failed to serialize settings: {}", e)))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, content).map_err(|e| {
            DomainError::Internal(format!("Failed to write settings tmp file: {}", e))
        })?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            DomainError::Internal(format!("Failed to publish settings file: {}", e))
        })?;
        Ok(())
    }
}
