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
    pub max_file_size_bytes: u64,
    pub default_bitrate_kbps: u32,
    pub export_wav: bool,
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
    pub models: ModelsConfig,
    pub audio: AudioConfig,
    pub auto_cleanup: bool,
    pub debug_mode: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            models: ModelsConfig::default(),
            audio: AudioConfig::default(),
            auto_cleanup: true,
            debug_mode: false,
        }
    }
}
