use super::language::LanguageId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordTimestamp {
    pub word: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub speaker_id: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub words: Vec<WordTimestamp>,
}

impl TranscriptSegment {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    pub language: LanguageId,
    pub segments: Vec<TranscriptSegment>,
}

impl Transcript {
    pub fn new(language: LanguageId, segments: Vec<TranscriptSegment>) -> Self {
        Self { language, segments }
    }

    pub fn total_duration_ms(&self) -> u64 {
        self.segments.last().map(|s| s.end_ms).unwrap_or(0)
    }

    pub fn unique_speakers(&self) -> Vec<String> {
        let mut speakers = HashSet::new();
        for segment in &self.segments {
            if let Some(ref speaker) = segment.speaker_id {
                speakers.insert(speaker.clone());
            }
        }
        let mut list: Vec<String> = speakers.into_iter().collect();
        list.sort();
        list
    }

    pub fn speaker_count(&self) -> usize {
        let count = self.unique_speakers().len();
        if count == 0 {
            1
        } else {
            count
        }
    }
}
