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
