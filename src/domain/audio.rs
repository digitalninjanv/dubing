use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFormat {
    Mp3,
    Wav,
    M4a,
    Aac,
    Ogg,
    Flac,
    Webm,
    Opus,
    Mp4,
    Mkv,
    Mov,
}

impl AudioFormat {
    pub fn is_video(&self) -> bool {
        matches!(self, Self::Mp4 | Self::Mkv | Self::Mov | Self::Webm)
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "mp3" => Some(Self::Mp3),
            "wav" => Some(Self::Wav),
            "m4a" => Some(Self::M4a),
            "aac" => Some(Self::Aac),
            "ogg" | "oga" => Some(Self::Ogg),
            "flac" => Some(Self::Flac),
            "webm" => Some(Self::Webm),
            "opus" => Some(Self::Opus),
            "mp4" | "m4v" => Some(Self::Mp4),
            "mkv" => Some(Self::Mkv),
            "mov" => Some(Self::Mov),
            _ => None,
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mp3",
            Self::Wav => "audio/wav",
            Self::M4a => "audio/m4a",
            Self::Aac => "audio/aac",
            Self::Ogg => "audio/ogg",
            Self::Flac => "audio/flac",
            Self::Webm => "audio/webm",
            Self::Opus => "audio/opus",
            Self::Mp4 => "video/mp4",
            Self::Mkv => "video/x-matroska",
            Self::Mov => "video/quicktime",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::M4a => "m4a",
            Self::Aac => "aac",
            Self::Ogg => "ogg",
            Self::Flac => "flac",
            Self::Webm => "webm",
            Self::Opus => "opus",
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Mov => "mov",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub codec: String,
    pub bitrate: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDocument {
    pub id: String,
    pub path: PathBuf,
    pub format: AudioFormat,
    pub mime_type: String,
    pub size_bytes: u64,
    pub metadata: MediaMetadata,
}

impl AudioDocument {
    pub fn validate_for_processing(&self, max_bytes: u64) -> Result<(), String> {
        if self.size_bytes == 0 {
            return Err("Audio file is empty (0 bytes)".to_string());
        }
        if self.size_bytes > max_bytes {
            return Err(format!(
                "Audio file size ({} MB) exceeds maximum allowed ({} MB)",
                self.size_bytes / (1024 * 1024),
                max_bytes / (1024 * 1024)
            ));
        }
        if self.metadata.duration_ms == 0 {
            return Err("Audio file has zero duration or cannot be decoded".to_string());
        }
        Ok(())
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio")
            .to_string()
    }
}
