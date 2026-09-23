use crate::domain::DomainError;
use crate::infrastructure::filesystem::{write_atomic, AppPaths};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default = "default_gemini_provider")]
    pub transcriber: String,
    #[serde(default = "default_gemini_provider")]
    pub translator: String,
    #[serde(default = "default_gemini_provider")]
    pub tts: String,
}

fn default_gemini_provider() -> String {
    "gemini".to_string()
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            transcriber: default_gemini_provider(),
            translator: default_gemini_provider(),
            tts: default_gemini_provider(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    #[serde(default = "default_transcriber_model")]
    pub transcriber: String,
    #[serde(default = "default_transcriber_fallbacks")]
    pub transcriber_fallbacks: Vec<String>,
    #[serde(default = "default_translator_model")]
    pub translator: String,
    #[serde(default = "default_translator_fallbacks")]
    pub translator_fallbacks: Vec<String>,
    #[serde(default = "default_tts_model")]
    pub tts: String,
    #[serde(default = "default_tts_fallbacks")]
    pub tts_fallbacks: Vec<String>,
}

fn default_transcriber_model() -> String {
    "gemini-3.5-transcribe".to_string()
}

fn default_transcriber_fallbacks() -> Vec<String> {
    vec![
        "gemini-3.8-flash".to_string(),
        "gemini-3.5-flash".to_string(),
    ]
}

fn default_translator_model() -> String {
    "gemini-3.5-flash-lite".to_string()
}

fn default_translator_fallbacks() -> Vec<String> {
    vec![
        "gemini-3.8-flash".to_string(),
        "gemini-3.5-flash".to_string(),
    ]
}

fn default_tts_model() -> String {
    "gemini-3.1-flash-tts-preview".to_string()
}

fn default_tts_fallbacks() -> Vec<String> {
    vec![
        "gemini-2.5-flash-preview-tts".to_string(),
        "gemini-2.5-pro-preview-tts".to_string(),
    ]
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            transcriber: default_transcriber_model(),
            transcriber_fallbacks: default_transcriber_fallbacks(),
            translator: default_translator_model(),
            translator_fallbacks: default_translator_fallbacks(),
            tts: default_tts_model(),
            tts_fallbacks: default_tts_fallbacks(),
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

fn default_tts_concurrency() -> usize {
    2
}

fn default_tts_spacing_ms() -> u64 {
    120
}

fn default_translation_concurrency() -> usize {
    3
}

fn default_translation_batch_size() -> usize {
    20
}

fn default_ffmpeg_concurrency() -> usize {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_tts_concurrency")]
    pub tts_concurrency: usize,
    #[serde(default = "default_tts_spacing_ms")]
    pub tts_request_spacing_ms: u64,
    #[serde(default = "default_translation_concurrency")]
    pub translation_concurrency: usize,
    #[serde(default = "default_translation_batch_size")]
    pub translation_batch_size: usize,
    #[serde(default = "default_ffmpeg_concurrency")]
    pub ffmpeg_concurrency: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            tts_concurrency: default_tts_concurrency(),
            tts_request_spacing_ms: default_tts_spacing_ms(),
            translation_concurrency: default_translation_concurrency(),
            translation_batch_size: default_translation_batch_size(),
            ffmpeg_concurrency: default_ffmpeg_concurrency(),
        }
    }
}

impl RuntimeConfig {
    pub fn normalized(mut self) -> Self {
        self.tts_concurrency = self.tts_concurrency.clamp(1, 8);
        self.tts_request_spacing_ms = self.tts_request_spacing_ms.min(5_000);
        self.translation_concurrency = self.translation_concurrency.clamp(1, 8);
        self.translation_batch_size = self.translation_batch_size.clamp(1, 50);
        self.ffmpeg_concurrency = self.ffmpeg_concurrency.clamp(1, 4);
        self
    }
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
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub models: ModelsConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
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
            providers: ProvidersConfig::default(),
            models: ModelsConfig::default(),
            audio: AudioConfig::default(),
            runtime: RuntimeConfig::default(),
            auto_cleanup: true,
            debug_mode: false,
            job_retention_days: default_retention_days(),
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.runtime = self.runtime.normalized();
        self
    }

    /// Load from `~/.config/audiodub/config.toml`; missing/corrupt file
    /// falls back to defaults (never fails startup).
    pub fn load() -> Self {
        let path = AppPaths::config_file();
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<AppSettings>(&content) {
                Ok(s) => s.normalized(),
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
        let normalized = self.clone().normalized();
        let content = toml::to_string_pretty(&normalized)
            .map_err(|e| DomainError::Internal(format!("Failed to serialize settings: {}", e)))?;
        write_atomic(&path, content.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::RuntimeConfig;

    #[test]
    fn runtime_limits_never_allow_zero_or_unbounded_values() {
        let config = RuntimeConfig {
            tts_concurrency: 0,
            tts_request_spacing_ms: u64::MAX,
            translation_concurrency: 99,
            translation_batch_size: 0,
            ffmpeg_concurrency: 99,
        }
        .normalized();

        assert_eq!(config.tts_concurrency, 1);
        assert_eq!(config.tts_request_spacing_ms, 5_000);
        assert_eq!(config.translation_concurrency, 8);
        assert_eq!(config.translation_batch_size, 1);
        assert_eq!(config.ffmpeg_concurrency, 4);
    }

    #[test]
    fn defaults_include_resilient_current_model_fallbacks() {
        let settings = super::AppSettings::default();
        assert_eq!(settings.models.transcriber, "gemini-3.5-transcribe");
        assert_eq!(settings.models.transcriber_fallbacks[0], "gemini-3.8-flash");
        assert_eq!(settings.models.translator, "gemini-3.5-flash-lite");
        assert_eq!(settings.models.translator_fallbacks[0], "gemini-3.8-flash");
        assert_eq!(
            settings.models.tts_fallbacks[0],
            "gemini-2.5-flash-preview-tts"
        );
        assert_eq!(
            settings.models.tts_fallbacks[1],
            "gemini-2.5-pro-preview-tts"
        );
    }
}
