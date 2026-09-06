use super::translation::TranslatedDocument;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    BilingualTxt,
}

impl SubtitleFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Vtt => "vtt",
            Self::BilingualTxt => "txt",
        }
    }
}

pub fn format_timestamp_srt(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let mins = (ms % 3_600_000) / 60_000;
    let secs = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{:02}:{:02}:{:02},{:03}", hours, mins, secs, millis)
}

pub fn format_timestamp_vtt(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let mins = (ms % 3_600_000) / 60_000;
    let secs = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
}

pub fn generate_srt(doc: &TranslatedDocument) -> String {
    let mut out = String::new();
    for (idx, seg) in doc.segments.iter().enumerate() {
        let num = idx + 1;
        let start = format_timestamp_srt(seg.source_start_ms);
        let end = format_timestamp_srt(seg.source_end_ms);
        let text = &seg.translated_text;

        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", num, start, end, text));
    }
    out
}

pub fn generate_vtt(doc: &TranslatedDocument) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for (idx, seg) in doc.segments.iter().enumerate() {
        let num = idx + 1;
        let start = format_timestamp_vtt(seg.source_start_ms);
        let end = format_timestamp_vtt(seg.source_end_ms);
        let text = &seg.translated_text;

        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", num, start, end, text));
    }
    out
}

pub fn generate_bilingual_txt(doc: &TranslatedDocument) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# AudioDub AI Bilingual Transcript\n# Source Language: {}\n# Target Language: {}\n\n",
        doc.source_language.as_str(),
        doc.target_language.as_str()
    ));

    for seg in &doc.segments {
        let speaker = seg.speaker_id.as_deref().unwrap_or("Speaker 1");
        let start = format_timestamp_vtt(seg.source_start_ms);
        let end = format_timestamp_vtt(seg.source_end_ms);

        out.push_str(&format!(
            "[{}] [{}] ({})\nOriginal  : {}\nTranslated: {}\n\n",
            start, end, speaker, seg.source_text, seg.translated_text
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LanguageId, TranslationSegment};

    #[test]
    fn test_format_timestamps() {
        assert_eq!(format_timestamp_srt(0), "00:00:00,000");
        assert_eq!(format_timestamp_srt(1500), "00:00:01,500");
        assert_eq!(format_timestamp_srt(3661234), "01:01:01,234");

        assert_eq!(format_timestamp_vtt(0), "00:00:00.000");
        assert_eq!(format_timestamp_vtt(1500), "00:00:01.500");
        assert_eq!(format_timestamp_vtt(3661234), "01:01:01.234");
    }

    #[test]
    fn test_generate_srt_and_vtt() {
        let doc = TranslatedDocument::new(
            LanguageId::new("id"),
            LanguageId::new("en"),
            vec![
                TranslationSegment {
                    segment_id: "seg_0001".to_string(),
                    speaker_id: Some("Speaker 1".to_string()),
                    source_start_ms: 0,
                    source_end_ms: 2500,
                    source_text: "Halo dunia".to_string(),
                    translated_text: "Hello world".to_string(),
                },
                TranslationSegment {
                    segment_id: "seg_0002".to_string(),
                    speaker_id: Some("Speaker 2".to_string()),
                    source_start_ms: 2600,
                    source_end_ms: 5000,
                    source_text: "Selamat pagi".to_string(),
                    translated_text: "Good morning".to_string(),
                },
            ],
        );

        let srt = generate_srt(&doc);
        assert!(srt.contains("1\n00:00:00,000 --> 00:00:02,500\nHello world"));
        assert!(srt.contains("2\n00:00:02,600 --> 00:00:05,000\nGood morning"));

        let vtt = generate_vtt(&doc);
        assert!(vtt.starts_with("WEBVTT\n\n"));
        assert!(vtt.contains("1\n00:00:00.000 --> 00:00:02.500\nHello world"));

        let txt = generate_bilingual_txt(&doc);
        assert!(txt.contains("Original  : Halo dunia"));
        assert!(txt.contains("Translated: Hello world"));
        assert!(txt.contains("(Speaker 1)"));
    }
}
