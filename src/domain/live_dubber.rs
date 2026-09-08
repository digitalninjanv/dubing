use serde::{Deserialize, Serialize};

/// Supported Gemini real-time models for Live Dubbing & Interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LiveModelChoice {
    /// Dedicated real-time speech-to-speech translation model (Google's premier engine for simultaneous interpretation).
    #[default]
    Gemini35LiveTranslate,
    /// Conversational dialog model with native audio (recommended fallback).
    Gemini31FlashLive,
    /// Native audio preview model (dialog baseline).
    Gemini25FlashNativeAudio,
    /// Real-time speech-to-text model for live subtitles.
    Gemini35TranscribeLive,
}

impl LiveModelChoice {
    pub fn model_id(&self) -> &'static str {
        match self {
            Self::Gemini35LiveTranslate => "gemini-3.5-live-translate-preview",
            Self::Gemini31FlashLive => "gemini-3.1-flash-live-preview",
            Self::Gemini25FlashNativeAudio => "gemini-2.5-flash-preview-native-audio-dialog",
            Self::Gemini35TranscribeLive => "gemini-3.5-transcribe-live",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Gemini35LiveTranslate => "Gemini 3.5 Live Translate (Primary S2S)",
            Self::Gemini31FlashLive => "Gemini 3.1 Flash Live (Conversational Fallback)",
            Self::Gemini25FlashNativeAudio => "Gemini 2.5 Flash Native Audio",
            Self::Gemini35TranscribeLive => "Gemini 3.5 Transcribe Live (Subtitles Only)",
        }
    }

    pub fn is_audio_output(&self) -> bool {
        !matches!(self, Self::Gemini35TranscribeLive)
    }
}

/// Source mode for capturing live audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AudioSourceMode {
    /// Intercepts YouTube / Browser sound via a virtual null sink, muting original audio from speakers.
    #[default]
    BrowserYouTube,
    /// Captures all desktop sound from the system audio monitor.
    SystemDesktop,
    /// Captures live voice from the physical microphone.
    Microphone,
}

impl AudioSourceMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::BrowserYouTube => "Browser / YouTube (Silences Original Audio)",
            Self::SystemDesktop => "System Desktop Monitor (All Audio)",
            Self::Microphone => "Microphone (Live Speech)",
        }
    }
}

/// Live lifecycle state of the Live Dubber session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiveDubberStatus {
    Idle,
    Initializing,
    RoutingAudio,
    Streaming,
    Translating,
    Playing,
    Stopped,
    Error(String),
}

impl LiveDubberStatus {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Initializing
                | Self::RoutingAudio
                | Self::Streaming
                | Self::Translating
                | Self::Playing
        )
    }

    pub fn display_status(&self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::Initializing => "Initializing Live Engine...",
            Self::RoutingAudio => "Silencing Original Audio & Routing...",
            Self::Streaming => "Listening to Audio Stream...",
            Self::Translating => "Translating Speech in Real-Time...",
            Self::Playing => "Playing Dubbed Voice to Speakers...",
            Self::Stopped => "Stopped",
            Self::Error(_) => "Error Encountered",
        }
    }
}

/// Information about a detected playback application in the audio server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioAppInfo {
    pub sink_input_id: u32,
    pub application_name: String,
    pub binary_name: String,
    pub media_name: Option<String>,
}

impl AudioAppInfo {
    pub fn is_browser(&self) -> bool {
        let app_lower = self.application_name.to_lowercase();
        let bin_lower = self.binary_name.to_lowercase();
        let media_lower = self.media_name.as_deref().unwrap_or("").to_lowercase();

        const BROWSER_KEYWORDS: &[&str] = &[
            "chrome", "firefox", "chromium", "brave", "msedge", "edge", "opera", "vivaldi",
            "epiphany", "webkit", "zen", "youtube", "browser", "gecko",
        ];

        BROWSER_KEYWORDS
            .iter()
            .any(|&k| app_lower.contains(k) || bin_lower.contains(k) || media_lower.contains(k))
    }
}

/// Real-time transcript event emitted during live interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveTranscriptUpdate {
    pub original_chunk: Option<String>,
    pub translated_chunk: Option<String>,
    pub is_turn_complete: bool,
    pub timestamp_ms: u64,
}
