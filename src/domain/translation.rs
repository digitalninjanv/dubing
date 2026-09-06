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
