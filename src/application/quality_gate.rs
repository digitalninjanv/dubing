use crate::domain::{
    AlignmentResult, AudioArtifact, DomainError, SynthesizedSegment, Transcript, TranslatedDocument,
};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub struct QualityGate;

impl QualityGate {
    pub fn validate_transcript(transcript: &Transcript) -> Result<(), DomainError> {
        if transcript.segments.is_empty() {
            return Err(Self::fail("Transcript contains no speech segments"));
        }

        let mut ids = HashSet::with_capacity(transcript.segments.len());
        let mut previous_start = 0u64;

        for (index, segment) in transcript.segments.iter().enumerate() {
            if segment.id.trim().is_empty() {
                return Err(Self::fail(format!(
                    "Transcript segment {} has an empty id",
                    index
                )));
            }
            if !ids.insert(segment.id.as_str()) {
                return Err(Self::fail(format!(
                    "Transcript contains duplicate segment id '{}'",
                    segment.id
                )));
            }
            if segment.start_ms >= segment.end_ms {
                return Err(Self::fail(format!(
                    "Transcript segment '{}' has invalid timing {}..{}",
                    segment.id, segment.start_ms, segment.end_ms
                )));
            }
            if segment.start_ms < previous_start {
                return Err(Self::fail(
                    "Transcript segments are not ordered by start time",
                ));
            }
            if segment.text.trim().is_empty() {
                return Err(Self::fail(format!(
                    "Transcript segment '{}' contains empty text",
                    segment.id
                )));
            }

            let mut previous_word_end = segment.start_ms;
            for word in &segment.words {
                if word.word.trim().is_empty() {
                    return Err(Self::fail(format!(
                        "Transcript segment '{}' contains an empty word timestamp",
                        segment.id
                    )));
                }
                if word.start_ms < segment.start_ms
                    || word.end_ms > segment.end_ms
                    || word.start_ms >= word.end_ms
                {
                    return Err(Self::fail(format!(
                        "Transcript segment '{}' contains an out-of-range word timestamp {}..{}",
                        segment.id, word.start_ms, word.end_ms
                    )));
                }
                if word.start_ms < previous_word_end {
                    return Err(Self::fail(format!(
                        "Transcript segment '{}' contains unordered word timestamps",
                        segment.id
                    )));
                }
                previous_word_end = word.end_ms;
            }

            previous_start = segment.start_ms;
        }

        Ok(())
    }

    pub fn validate_translation(
        transcript: &Transcript,
        translated: &TranslatedDocument,
    ) -> Result<(), DomainError> {
        Self::validate_transcript(transcript)?;

        if translated.segments.len() != transcript.segments.len() {
            return Err(Self::fail(format!(
                "Translation count mismatch: transcript={}, translation={}",
                transcript.segments.len(),
                translated.segments.len()
            )));
        }

        let expected: std::collections::HashMap<&str, (&str, u64, u64)> = transcript
            .segments
            .iter()
            .map(|segment| {
                (
                    segment.id.as_str(),
                    (segment.text.as_str(), segment.start_ms, segment.end_ms),
                )
            })
            .collect();

        let mut seen = HashSet::with_capacity(translated.segments.len());

        for segment in &translated.segments {
            if !seen.insert(segment.segment_id.as_str()) {
                return Err(Self::fail(format!(
                    "Translation contains duplicate segment id '{}'",
                    segment.segment_id
                )));
            }

            let Some((source_text, start_ms, end_ms)) = expected.get(segment.segment_id.as_str())
            else {
                return Err(Self::fail(format!(
                    "Translation references unknown segment id '{}'",
                    segment.segment_id
                )));
            };

            if segment.source_start_ms != *start_ms || segment.source_end_ms != *end_ms {
                return Err(Self::fail(format!(
                    "Translation timing mismatch for segment '{}'",
                    segment.segment_id
                )));
            }

            if segment.source_text.trim().is_empty()
                || segment.source_text.trim() != source_text.trim()
            {
                return Err(Self::fail(format!(
                    "Translation source text mismatch for segment '{}'",
                    segment.segment_id
                )));
            }

            if segment.translated_text.trim().is_empty() {
                return Err(Self::fail(format!(
                    "Translation segment '{}' contains empty translated text",
                    segment.segment_id
                )));
            }
        }

        if seen.len() != expected.len() {
            return Err(Self::fail(
                "Translation is missing one or more transcript segments",
            ));
        }

        Ok(())
    }

    pub fn validate_synthesis(
        translated: &TranslatedDocument,
        synthesized: &[SynthesizedSegment],
    ) -> Result<(), DomainError> {
        if synthesized.len() != translated.segments.len() {
            return Err(Self::fail(format!(
                "Synthesis count mismatch: translation={}, synthesis={}",
                translated.segments.len(),
                synthesized.len()
            )));
        }

        let expected: HashSet<&str> = translated
            .segments
            .iter()
            .map(|segment| segment.segment_id.as_str())
            .collect();
        let mut seen = HashSet::with_capacity(synthesized.len());

        for segment in synthesized {
            if !seen.insert(segment.segment_id.as_str()) {
                return Err(Self::fail(format!(
                    "Synthesis contains duplicate segment id '{}'",
                    segment.segment_id
                )));
            }
            if !expected.contains(segment.segment_id.as_str()) {
                return Err(Self::fail(format!(
                    "Synthesis references unknown segment id '{}'",
                    segment.segment_id
                )));
            }
            if segment.duration_ms == 0 {
                return Err(Self::fail(format!(
                    "Synthesis segment '{}' has zero duration",
                    segment.segment_id
                )));
            }
            Self::validate_file(
                &segment.path,
                &format!("synthesis '{}'", segment.segment_id),
            )?;
        }

        Ok(())
    }

    pub fn validate_alignment(
        synthesized: &[SynthesizedSegment],
        alignment: &AlignmentResult,
    ) -> Result<(), DomainError> {
        if alignment.aligned_files.len() < synthesized.len() {
            return Err(Self::fail(format!(
                "Alignment produced too few files: synthesis={}, alignment={}",
                synthesized.len(),
                alignment.aligned_files.len()
            )));
        }

        for (index, path) in alignment.aligned_files.iter().enumerate() {
            Self::validate_file(path, &format!("aligned segment {}", index))?;
        }

        Ok(())
    }

    pub fn validate_output(artifact: &AudioArtifact) -> Result<(), DomainError> {
        if artifact.duration_ms == 0 {
            return Err(Self::fail("Final output has zero duration"));
        }
        if artifact.size_bytes == 0 {
            return Err(Self::fail("Final output is empty"));
        }
        Self::validate_file(&artifact.path, "final output")?;
        Ok(())
    }

    fn validate_file(path: &Path, label: &str) -> Result<(), DomainError> {
        let metadata = fs::metadata(path).map_err(|e| {
            Self::fail(format!(
                "{} '{}' cannot be inspected: {}",
                label,
                path.display(),
                e
            ))
        })?;

        if !metadata.is_file() || metadata.len() == 0 {
            return Err(Self::fail(format!(
                "{} '{}' is missing or empty",
                label,
                path.display()
            )));
        }

        Ok(())
    }

    fn fail(message: impl Into<String>) -> DomainError {
        DomainError::QualityGate(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::QualityGate;
    use crate::domain::{
        LanguageId, Transcript, TranscriptSegment, TranslatedDocument, TranslationSegment,
    };

    fn transcript() -> Transcript {
        Transcript::new(
            LanguageId::new("en"),
            vec![
                TranscriptSegment {
                    id: "s1".to_string(),
                    speaker_id: None,
                    start_ms: 0,
                    end_ms: 1000,
                    text: "Hello".to_string(),
                    words: Vec::new(),
                },
                TranscriptSegment {
                    id: "s2".to_string(),
                    speaker_id: Some("S1".to_string()),
                    start_ms: 1000,
                    end_ms: 2000,
                    text: "World".to_string(),
                    words: Vec::new(),
                },
            ],
        )
    }

    fn translation() -> TranslatedDocument {
        TranslatedDocument::new(
            LanguageId::new("en"),
            LanguageId::new("id"),
            vec![
                TranslationSegment {
                    segment_id: "s1".to_string(),
                    speaker_id: None,
                    source_start_ms: 0,
                    source_end_ms: 1000,
                    source_text: "Hello".to_string(),
                    translated_text: "Halo".to_string(),
                },
                TranslationSegment {
                    segment_id: "s2".to_string(),
                    speaker_id: Some("S1".to_string()),
                    source_start_ms: 1000,
                    source_end_ms: 2000,
                    source_text: "World".to_string(),
                    translated_text: "Dunia".to_string(),
                },
            ],
        )
    }

    #[test]
    fn accepts_consistent_transcript() {
        assert!(QualityGate::validate_transcript(&transcript()).is_ok());
    }

    #[test]
    fn rejects_duplicate_transcript_ids() {
        let mut value = transcript();
        value.segments[1].id = "s1".to_string();

        assert!(QualityGate::validate_transcript(&value).is_err());
    }

    #[test]
    fn rejects_translation_with_missing_segment() {
        let mut value = translation();
        value.segments.pop();

        assert!(QualityGate::validate_translation(&transcript(), &value).is_err());
    }

    #[test]
    fn rejects_mismatched_translation_timing() {
        let mut value = translation();
        value.segments[1].source_end_ms = 2100;

        assert!(QualityGate::validate_translation(&transcript(), &value).is_err());
    }


    #[test]
    fn rejects_word_timestamp_outside_segment() {
        let mut value = transcript();
        value.segments[0].words.push(crate::domain::WordTimestamp {
            word: "Hello".to_string(),
            start_ms: 0,
            end_ms: 1200,
        });

        assert!(QualityGate::validate_transcript(&value).is_err());
    }

    #[test]
    fn rejects_unordered_word_timestamps() {
        let mut value = transcript();
        value.segments[0].words = vec![
            crate::domain::WordTimestamp {
                word: "Hello".to_string(),
                start_ms: 100,
                end_ms: 500,
            },
            crate::domain::WordTimestamp {
                word: "again".to_string(),
                start_ms: 400,
                end_ms: 900,
            },
        ];

        assert!(QualityGate::validate_transcript(&value).is_err());
    }
}
