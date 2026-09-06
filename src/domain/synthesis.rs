use super::audio::AudioFormat;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub id: String,
    pub voice_name: String,
    pub language: String,
    pub style: Option<String>,
    pub speed: f32,
}

impl Default for VoiceProfile {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            voice_name: "Kore".to_string(),
            language: "en".to_string(),
            style: None,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesizedSegment {
    pub segment_id: String,
    pub speaker_id: Option<String>,
    pub path: PathBuf,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioArtifact {
    pub path: PathBuf,
    pub format: AudioFormat,
    pub duration_ms: u64,
    pub size_bytes: u64,
    pub quality_warnings: Vec<String>,
    pub subtitle_srt_path: Option<PathBuf>,
    pub subtitle_vtt_path: Option<PathBuf>,
    pub transcript_txt_path: Option<PathBuf>,
    pub video_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlignmentResult {
    pub aligned_files: Vec<PathBuf>,
    pub quality_warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SpeakerVoiceConfig {
    pub speaker_1_voice: Option<String>,
    pub speaker_2_voice: Option<String>,
}

impl SpeakerVoiceConfig {
    pub fn new(s1: Option<String>, s2: Option<String>) -> Self {
        Self {
            speaker_1_voice: s1.filter(|s| !s.trim().is_empty()),
            speaker_2_voice: s2.filter(|s| !s.trim().is_empty()),
        }
    }

    pub fn get_voice_for(&self, speaker_id: Option<&str>) -> Option<&str> {
        match speaker_id {
            Some(s) if s.contains('2') => self.speaker_2_voice.as_deref(),
            _ => self.speaker_1_voice.as_deref(),
        }
    }
}
