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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TtsStylePreset {
    #[default]
    Natural,
    Storyteller,
    NewsBroadcaster,
    Energetic,
    Calm,
    Custom(String),
}

impl TtsStylePreset {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Natural => "Natural & Conversational",
            Self::Storyteller => "Storyteller (Dramatic & Expressive)",
            Self::NewsBroadcaster => "News Broadcaster (Formal & Clear)",
            Self::Energetic => "Energetic & Cheerful (High Energy)",
            Self::Calm => "Calm & Meditative (Soft & Soothing)",
            Self::Custom(_) => "Custom Instruction (AI Studio Style)",
        }
    }

    pub fn prompt_directive(&self, custom_input: Option<&str>) -> String {
        match self {
            Self::Natural => {
                "Speak in a natural, clear, and conversational speaking tone with relaxed pacing."
                    .to_string()
            }
            Self::Storyteller => {
                "Speak like an expressive and captivating storyteller, with dramatic pauses, emotional inflection, and dynamic vocal range."
                    .to_string()
            }
            Self::NewsBroadcaster => {
                "Speak in a professional, authoritative, and articulate news broadcaster style with steady cadence and clear enunciation."
                    .to_string()
            }
            Self::Energetic => {
                "Speak with high energy, enthusiasm, and an upbeat, friendly tone suitable for a marketing showcase or podcast intro."
                    .to_string()
            }
            Self::Calm => {
                "Speak in a calm, gentle, and soothing tone with slower, relaxing pacing and a warm vocal resonance."
                    .to_string()
            }
            Self::Custom(s) => {
                let instruction = custom_input.unwrap_or(s.as_str()).trim();
                if instruction.is_empty() {
                    "Speak clearly and naturally.".to_string()
                } else {
                    instruction.to_string()
                }
            }
        }
    }

    pub fn from_index(idx: u32, custom: Option<String>) -> Self {
        match idx {
            1 => Self::Storyteller,
            2 => Self::NewsBroadcaster,
            3 => Self::Energetic,
            4 => Self::Calm,
            5 => Self::Custom(custom.unwrap_or_default()),
            _ => Self::Natural,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TtsRequest {
    pub text: String,
    pub voice: VoiceProfile,
    pub style_preset: TtsStylePreset,
    pub custom_style: Option<String>,
    pub speed: f32,
}

impl TtsRequest {
    pub fn new(text: String, voice: VoiceProfile) -> Self {
        Self {
            text,
            voice,
            style_preset: TtsStylePreset::Natural,
            custom_style: None,
            speed: 1.0,
        }
    }

    pub fn effective_style_instruction(&self) -> String {
        self.style_preset
            .prompt_directive(self.custom_style.as_deref())
    }
}
