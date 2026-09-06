use super::language::LanguageId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationSegment {
    pub segment_id: String,
    pub speaker_id: Option<String>,
    pub source_start_ms: u64,
    pub source_end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
}

impl TranslationSegment {
    pub fn target_duration_ms(&self) -> u64 {
        self.source_end_ms.saturating_sub(self.source_start_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslatedDocument {
    pub source_language: LanguageId,
    pub target_language: LanguageId,
    pub segments: Vec<TranslationSegment>,
}

impl TranslatedDocument {
    pub fn new(
        source_language: LanguageId,
        target_language: LanguageId,
        segments: Vec<TranslationSegment>,
    ) -> Self {
        Self {
            source_language,
            target_language,
            segments,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TranslationTone {
    #[default]
    Neutral,
    Casual,
    Formal,
    Creative,
}

impl TranslationTone {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Casual => "casual",
            Self::Formal => "formal",
            Self::Creative => "creative",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Neutral => "Neutral (Default)",
            Self::Casual => "Casual & Conversational",
            Self::Formal => "Formal & Polite",
            Self::Creative => "Creative & Expressive",
        }
    }

    pub fn prompt_directive(&self) -> &'static str {
        match self {
            Self::Neutral => "Use a clear, natural, neutral, and standard vocabulary balanced for general audiences.",
            Self::Casual => "Translate into a natural, conversational, everyday speaking style with casual phrasing, friendly tone, and colloquial expressions suitable for podcasts or informal vlogs.",
            Self::Formal => "Translate into a respectful, polite, and grammatically formal style suitable for business presentations, academic lectures, or official speeches.",
            Self::Creative => "Translate with creative, engaging, dynamic, and expressive vocabulary, preserving idiomatic nuance, emotion, and rhythm.",
        }
    }

    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().trim() {
            "casual" | "relaxed" | "conversational" => Self::Casual,
            "formal" | "polite" | "business" => Self::Formal,
            "creative" | "dramatic" | "expressive" => Self::Creative,
            _ => Self::Neutral,
        }
    }
}
