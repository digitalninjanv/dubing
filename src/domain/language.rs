use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LanguageId(String);

impl LanguageId {
    pub const AUTO: &'static str = "auto";

    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into().to_lowercase())
    }

    pub fn auto() -> Self {
        Self(Self::AUTO.to_string())
    }

    pub fn is_auto(&self) -> bool {
        self.0 == Self::AUTO
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Converts language code to ISO 639-2 3-letter code for FFmpeg soft subtitle tracks
    pub fn to_iso639_2(&self) -> &'static str {
        match self.0.as_str() {
            "en" => "eng",
            "id" => "ind",
            "es" => "spa",
            "fr" => "fra",
            "de" => "deu",
            "ja" => "jpn",
            "ko" => "kor",
            "zh" => "zho",
            "ar" => "ara",
            "ru" => "rus",
            "pt" => "por",
            "it" => "ita",
            "hi" => "hin",
            _ => "und",
        }
    }
}

impl std::fmt::Display for LanguageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for LanguageId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for LanguageId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub id: LanguageId,
    pub locale: String,
    pub display_name: String,
    pub native_name: String,
    pub transcription_supported: bool,
    pub translation_supported: bool,
    pub tts_supported: bool,
    pub default_voice: String,
}

#[derive(Debug, Clone)]
pub struct LanguageRegistry {
    languages: HashMap<LanguageId, LanguageInfo>,
    ordered_ids: Vec<LanguageId>,
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

impl LanguageRegistry {
    pub fn new() -> Self {
        Self {
            languages: HashMap::new(),
            ordered_ids: Vec::new(),
        }
    }

    pub fn standard() -> Self {
        let mut registry = Self::new();

        // Populate with standard languages supported by Gemini 3.5 Transcribe, 3.1 Flash-Lite, and 3.1 Flash TTS
        let items = [
            (
                "id",
                "id-ID",
                "Indonesian",
                "Bahasa Indonesia",
                true,
                true,
                true,
                "Aoede",
            ),
            (
                "en", "en-US", "English", "English", true, true, true, "Kore",
            ),
            (
                "ja",
                "ja-JP",
                "Japanese",
                "日本語",
                true,
                true,
                true,
                "Kore",
            ),
            ("ko", "ko-KR", "Korean", "한국어", true, true, true, "Kore"),
            (
                "es", "es-ES", "Spanish", "Español", true, true, true, "Puck",
            ),
            (
                "jv",
                "jv-ID",
                "Javanese",
                "Basa Jawa",
                true,
                true,
                true,
                "Aoede",
            ),
            (
                "ms",
                "ms-MY",
                "Malay",
                "Bahasa Melayu",
                true,
                true,
                true,
                "Aoede",
            ),
            (
                "vi",
                "vi-VN",
                "Vietnamese",
                "Tiếng Việt",
                true,
                true,
                true,
                "Kore",
            ),
            (
                "zh",
                "zh-CN",
                "Chinese (Mandarin)",
                "中文",
                true,
                true,
                true,
                "Kore",
            ),
            (
                "de", "de-DE", "German", "Deutsch", true, true, true, "Fenrir",
            ),
            (
                "fr",
                "fr-FR",
                "French",
                "Français",
                true,
                true,
                true,
                "Puck",
            ),
        ];

        for (code, locale, display, native, stt, trans, tts, voice) in items {
            registry.register(LanguageInfo {
                id: LanguageId::new(code),
                locale: locale.to_string(),
                display_name: display.to_string(),
                native_name: native.to_string(),
                transcription_supported: stt,
                translation_supported: trans,
                tts_supported: tts,
                default_voice: voice.to_string(),
            });
        }

        registry
    }

    pub fn register(&mut self, info: LanguageInfo) {
        let id = info.id.clone();
        if !self.languages.contains_key(&id) {
            self.ordered_ids.push(id.clone());
        }
        self.languages.insert(id, info);
    }

    pub fn get(&self, id: &LanguageId) -> Option<&LanguageInfo> {
        self.languages.get(id)
    }

    pub fn list(&self) -> Vec<&LanguageInfo> {
        self.ordered_ids
            .iter()
            .filter_map(|id| self.languages.get(id))
            .collect()
    }

    pub fn can_transcribe(&self, id: &LanguageId) -> bool {
        if id.is_auto() {
            return true;
        }
        self.languages
            .get(id)
            .map(|l| l.transcription_supported)
            .unwrap_or(false)
    }

    pub fn can_translate(&self, id: &LanguageId) -> bool {
        self.languages
            .get(id)
            .map(|l| l.translation_supported)
            .unwrap_or(false)
    }

    pub fn can_synthesize(&self, id: &LanguageId) -> bool {
        self.languages
            .get(id)
            .map(|l| l.tts_supported)
            .unwrap_or(false)
    }

    pub fn validate_pair(&self, source: &LanguageId, target: &LanguageId) -> Result<(), String> {
        if source == target {
            return Err("Source and target languages cannot be identical".to_string());
        }
        if !self.can_transcribe(source) {
            return Err(format!(
                "Source language '{}' is not supported for transcription",
                source
            ));
        }
        if target.is_auto() {
            return Err("Target language cannot be 'auto'".to_string());
        }
        if !self.can_translate(target) {
            return Err(format!(
                "Target language '{}' is not supported for translation",
                target
            ));
        }
        if !self.can_synthesize(target) {
            return Err(format!(
                "Target language '{}' is not supported for speech synthesis (TTS)",
                target
            ));
        }
        Ok(())
    }
}
