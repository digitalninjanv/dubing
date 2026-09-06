use audiodub::domain::{
    AudioDocument, AudioFormat, Job, LanguageId, LanguageRegistry, MediaMetadata, PipelineStage,
    Transcript, TranscriptSegment, WordTimestamp,
};
use std::path::PathBuf;

#[test]
fn test_language_registry_standard() {
    let registry = LanguageRegistry::standard();
    assert!(registry.can_transcribe(&LanguageId::new("id")));
    assert!(registry.can_translate(&LanguageId::new("en")));
    assert!(registry.can_synthesize(&LanguageId::new("ja")));

    // Auto is allowed for transcribe
    assert!(registry.can_transcribe(&LanguageId::auto()));
    // Auto is NOT allowed for translate
    assert!(!registry.can_translate(&LanguageId::auto()));

    // Valid pair
    assert!(registry
        .validate_pair(&LanguageId::new("id"), &LanguageId::new("en"))
        .is_ok());

    // Identical pair must fail
    assert!(registry
        .validate_pair(&LanguageId::new("en"), &LanguageId::new("en"))
        .is_err());

    // Unknown language must fail
    assert!(registry
        .validate_pair(&LanguageId::new("xyz"), &LanguageId::new("en"))
        .is_err());
}

#[test]
fn test_audio_format_extensions() {
    assert_eq!(AudioFormat::from_extension("mp3"), Some(AudioFormat::Mp3));
    assert_eq!(AudioFormat::from_extension("WAV"), Some(AudioFormat::Wav));
    assert_eq!(AudioFormat::from_extension("flac"), Some(AudioFormat::Flac));
    assert_eq!(AudioFormat::from_extension("m4a"), Some(AudioFormat::M4a));
    assert_eq!(AudioFormat::from_extension("xyz"), None);

    assert_eq!(AudioFormat::Mp3.mime_type(), "audio/mp3");
}

#[test]
fn test_job_state_machine() {
    let dummy_audio = AudioDocument {
        id: "doc_1".to_string(),
        path: PathBuf::from("/tmp/test.mp3"),
        format: AudioFormat::Mp3,
        mime_type: "audio/mp3".to_string(),
        size_bytes: 1024,
        metadata: MediaMetadata {
            duration_ms: 10000,
            sample_rate: 44100,
            channels: 2,
            codec: "mp3".to_string(),
            bitrate: Some(192000),
        },
    };

    let mut job = Job::new(dummy_audio, LanguageId::new("id"), LanguageId::new("en"));
    assert_eq!(job.stage, PipelineStage::Idle);

    // Normal progression
    assert!(job.transition_to(PipelineStage::Validating).is_ok());
    assert!(job.transition_to(PipelineStage::Uploading).is_ok());
    assert!(job.transition_to(PipelineStage::Transcribing).is_ok());
    assert!(job.transition_to(PipelineStage::Translating).is_ok());
    assert!(job.transition_to(PipelineStage::Synthesizing).is_ok());
    assert!(job.transition_to(PipelineStage::Aligning).is_ok());
    assert!(job.transition_to(PipelineStage::Exporting).is_ok());
    assert!(job.transition_to(PipelineStage::ValidatingOutput).is_ok());
    assert!(job.transition_to(PipelineStage::Completed).is_ok());

    // Completed is terminal, cannot transition further
    assert!(job.transition_to(PipelineStage::Validating).is_err());
}

#[test]
fn test_job_retry_and_cancellation() {
    let dummy_audio = AudioDocument {
        id: "doc_1".to_string(),
        path: PathBuf::from("/tmp/test.mp3"),
        format: AudioFormat::Mp3,
        mime_type: "audio/mp3".to_string(),
        size_bytes: 1024,
        metadata: MediaMetadata {
            duration_ms: 10000,
            sample_rate: 44100,
            channels: 2,
            codec: "mp3".to_string(),
            bitrate: Some(192000),
        },
    };

    let mut job = Job::new(dummy_audio, LanguageId::new("id"), LanguageId::new("en"));
    job.transition_to(PipelineStage::Validating).unwrap();
    job.transition_to(PipelineStage::Uploading).unwrap();

    // Transient failure
    job.fail(true, "Rate limit hit".to_string());
    assert_eq!(job.stage, PipelineStage::FailedRetryable);

    // Retry allows resuming uploading
    assert!(job.transition_to(PipelineStage::Uploading).is_ok());

    // User cancellation
    job.cancel();
    assert_eq!(job.stage, PipelineStage::Cancelled);
    assert!(job.transition_to(PipelineStage::Uploading).is_err());
}

#[test]
fn test_transcript_speaker_and_timing() {
    let seg1 = TranscriptSegment {
        id: "seg_1".to_string(),
        speaker_id: Some("Speaker 1".to_string()),
        start_ms: 0,
        end_ms: 2500,
        text: "Halo semuanya.".to_string(),
        words: vec![
            WordTimestamp {
                word: "Halo".to_string(),
                start_ms: 0,
                end_ms: 800,
            },
            WordTimestamp {
                word: "semuanya.".to_string(),
                start_ms: 850,
                end_ms: 2500,
            },
        ],
    };

    let seg2 = TranscriptSegment {
        id: "seg_2".to_string(),
        speaker_id: Some("Speaker 2".to_string()),
        start_ms: 3000,
        end_ms: 5500,
        text: "Halo juga!".to_string(),
        words: vec![],
    };

    let transcript = Transcript::new(LanguageId::new("id"), vec![seg1, seg2]);
    assert_eq!(transcript.speaker_count(), 2);
    assert_eq!(
        transcript.unique_speakers(),
        vec!["Speaker 1".to_string(), "Speaker 2".to_string()]
    );
    assert_eq!(transcript.total_duration_ms(), 5500);
}

#[test]
fn test_video_format_detection() {
    assert!(AudioFormat::Mp4.is_video());
    assert!(AudioFormat::Mkv.is_video());
    assert!(AudioFormat::Mov.is_video());
    assert!(AudioFormat::Webm.is_video());

    assert!(!AudioFormat::Mp3.is_video());
    assert!(!AudioFormat::Wav.is_video());
    assert!(!AudioFormat::Flac.is_video());
    assert!(!AudioFormat::M4a.is_video());

    assert_eq!(AudioFormat::from_extension("mp4"), Some(AudioFormat::Mp4));
    assert_eq!(AudioFormat::from_extension("mkv"), Some(AudioFormat::Mkv));
    assert_eq!(AudioFormat::from_extension("mov"), Some(AudioFormat::Mov));
}

#[test]
fn test_translation_tone_directive() {
    use audiodub::domain::TranslationTone;

    assert_eq!(
        TranslationTone::from_str_loose("casual"),
        TranslationTone::Casual
    );
    assert_eq!(
        TranslationTone::from_str_loose("Formal"),
        TranslationTone::Formal
    );
    assert_eq!(
        TranslationTone::from_str_loose("dramatic"),
        TranslationTone::Creative
    );
    assert_eq!(
        TranslationTone::from_str_loose("unknown"),
        TranslationTone::Neutral
    );

    assert!(TranslationTone::Casual
        .prompt_directive()
        .contains("casual"));
    assert!(TranslationTone::Formal
        .prompt_directive()
        .contains("formal"));
    assert!(TranslationTone::Creative
        .prompt_directive()
        .contains("creative"));
    assert!(TranslationTone::Neutral
        .prompt_directive()
        .contains("neutral"));
}

#[test]
fn test_speaker_voice_config() {
    use audiodub::domain::SpeakerVoiceConfig;

    let cfg = SpeakerVoiceConfig::new(Some("Kore".to_string()), Some("Puck".to_string()));
    assert_eq!(cfg.get_voice_for(Some("Speaker 1")), Some("Kore"));
    assert_eq!(cfg.get_voice_for(Some("Speaker 2")), Some("Puck"));
    assert_eq!(cfg.get_voice_for(None), Some("Kore"));

    let empty_cfg = SpeakerVoiceConfig::default();
    assert_eq!(empty_cfg.get_voice_for(Some("Speaker 1")), None);
}

#[test]
fn test_subtitle_generation() {
    use audiodub::domain::{
        format_timestamp_srt, format_timestamp_vtt, generate_bilingual_txt, generate_srt,
        generate_vtt, LanguageId, TranslatedDocument, TranslationSegment,
    };

    assert_eq!(format_timestamp_srt(0), "00:00:00,000");
    assert_eq!(format_timestamp_srt(65432), "00:01:05,432");
    assert_eq!(format_timestamp_vtt(65432), "00:01:05.432");

    let doc = TranslatedDocument::new(
        LanguageId::new("id"),
        LanguageId::new("en"),
        vec![TranslationSegment {
            segment_id: "seg_1".to_string(),
            speaker_id: Some("Speaker 1".to_string()),
            source_start_ms: 1000,
            source_end_ms: 3500,
            source_text: "Halo dunia.".to_string(),
            translated_text: "Hello world.".to_string(),
        }],
    );

    let srt = generate_srt(&doc);
    assert!(srt.contains("1\n00:00:01,000 --> 00:00:03,500\nHello world."));

    let vtt = generate_vtt(&doc);
    assert!(vtt.starts_with("WEBVTT\n\n"));
    assert!(vtt.contains("00:00:01.000 --> 00:00:03.500\nHello world."));

    let txt = generate_bilingual_txt(&doc);
    assert!(txt.contains("Original  : Halo dunia."));
    assert!(txt.contains("Translated: Hello world."));
}
