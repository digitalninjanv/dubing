use async_trait::async_trait;
use audiodub::application::ports::{
    AudioEngine, SpeechSynthesizer, SpeechTranscriber, TextTranslator,
};
use audiodub::application::PipelineOrchestrator;
use audiodub::config::AudioConfig;
use audiodub::domain::{
    AudioDocument, AudioFormat, DomainError, Job, LanguageId, SynthesizedSegment, Transcript,
    TranscriptSegment, TranslatedDocument, TranslationSegment, VoiceProfile, WordTimestamp,
};
use audiodub::infrastructure::ffmpeg::FfmpegAudioEngine;
use audiodub::infrastructure::filesystem::FileJobRepository;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

// Mock Transcriber
struct MockTranscriber;
#[async_trait]
impl SpeechTranscriber for MockTranscriber {
    async fn transcribe(
        &self,
        _audio: &AudioDocument,
        _source_hint: &LanguageId,
    ) -> Result<Transcript, DomainError> {
        Ok(Transcript::new(
            LanguageId::new("id"),
            vec![
                TranscriptSegment {
                    id: "seg_0001".to_string(),
                    speaker_id: Some("Speaker 1".to_string()),
                    start_ms: 0,
                    end_ms: 1000,
                    text: "Selamat pagi.".to_string(),
                    words: vec![WordTimestamp {
                        word: "Selamat".to_string(),
                        start_ms: 0,
                        end_ms: 500,
                    }],
                },
                TranscriptSegment {
                    id: "seg_0002".to_string(),
                    speaker_id: Some("Speaker 2".to_string()),
                    start_ms: 1200,
                    end_ms: 2200,
                    text: "Pagi juga.".to_string(),
                    words: vec![],
                },
            ],
        ))
    }
}

// Mock Translator
struct MockTranslator;
#[async_trait]
impl TextTranslator for MockTranslator {
    async fn translate(
        &self,
        _transcript: &Transcript,
        target_lang: &LanguageId,
    ) -> Result<TranslatedDocument, DomainError> {
        Ok(TranslatedDocument::new(
            LanguageId::new("id"),
            target_lang.clone(),
            vec![
                TranslationSegment {
                    segment_id: "seg_0001".to_string(),
                    speaker_id: Some("Speaker 1".to_string()),
                    source_start_ms: 0,
                    source_end_ms: 1000,
                    source_text: "Selamat pagi.".to_string(),
                    translated_text: "Good morning.".to_string(),
                },
                TranslationSegment {
                    segment_id: "seg_0002".to_string(),
                    speaker_id: Some("Speaker 2".to_string()),
                    source_start_ms: 1200,
                    source_end_ms: 2200,
                    source_text: "Pagi juga.".to_string(),
                    translated_text: "Morning too.".to_string(),
                },
            ],
        ))
    }
}

// Mock Synthesizer that writes small valid tone audio files for testing
struct MockSynthesizer;
#[async_trait]
impl SpeechSynthesizer for MockSynthesizer {
    async fn synthesize_segment(
        &self,
        segment: &TranslationSegment,
        _voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("sine=frequency=440:duration=1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(output_path)
            .status()
            .map_err(|e| DomainError::Internal(format!("ffmpeg failed: {}", e)))?;

        if !status.success() {
            return Err(DomainError::Internal(
                "ffmpeg failed to generate mock voice".to_string(),
            ));
        }

        Ok(SynthesizedSegment {
            segment_id: segment.segment_id.clone(),
            speaker_id: segment.speaker_id.clone(),
            path: output_path.to_path_buf(),
            duration_ms: 1000,
        })
    }
}

#[tokio::test]
async fn test_pipeline_orchestrator_end_to_end() {
    let dir = tempdir().unwrap();
    let sample_mp3 = dir.path().join("input_podcast.mp3");

    // Generate real 2-second audio file
    let _ = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("sine=frequency=440:duration=2")
        .arg("-c:a")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg("128k")
        .arg(&sample_mp3)
        .status()
        .unwrap();

    let engine = Arc::new(FfmpegAudioEngine::new());
    let doc = engine
        .inspect_and_validate(&sample_mp3, 100 * 1024 * 1024)
        .await
        .unwrap();

    let job = Job::new(doc, LanguageId::auto(), LanguageId::new("en"));
    let job_repo = Arc::new(FileJobRepository::with_dir(dir.path().join("jobs")));

    let orchestrator = PipelineOrchestrator::new(
        Arc::new(MockTranscriber),
        Arc::new(MockTranslator),
        Arc::new(MockSynthesizer),
        engine,
        job_repo.clone(),
        AudioConfig::default(),
    );

    let cancel_token = CancellationToken::new();
    let progress_count = Arc::new(AtomicUsize::new(0));
    let progress_clone = progress_count.clone();

    let result = orchestrator
        .run_job(job, cancel_token, move |_updated_job| {
            progress_clone.fetch_add(1, Ordering::SeqCst);
        })
        .await;

    assert!(result.is_ok(), "Pipeline failed: {:?}", result.err());
    let artifact = result.unwrap();

    assert_eq!(artifact.format, AudioFormat::Mp3);
    assert!(artifact.duration_ms > 0);
    assert!(artifact.size_bytes > 0);
    assert!(artifact.path.exists());
    assert!(progress_count.load(Ordering::SeqCst) >= 8);
}

#[tokio::test]
async fn test_pipeline_cancellation() {
    let dir = tempdir().unwrap();
    let sample_mp3 = dir.path().join("input_cancelled.mp3");

    let _ = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("sine=frequency=440:duration=1")
        .arg("-c:a")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg("128k")
        .arg(&sample_mp3)
        .status()
        .unwrap();

    let engine = Arc::new(FfmpegAudioEngine::new());
    let doc = engine
        .inspect_and_validate(&sample_mp3, 100 * 1024 * 1024)
        .await
        .unwrap();

    let job = Job::new(doc, LanguageId::auto(), LanguageId::new("en"));
    let job_repo = Arc::new(FileJobRepository::with_dir(dir.path().join("jobs")));

    let orchestrator = PipelineOrchestrator::new(
        Arc::new(MockTranscriber),
        Arc::new(MockTranslator),
        Arc::new(MockSynthesizer),
        engine,
        job_repo,
        AudioConfig::default(),
    );

    let cancel_token = CancellationToken::new();
    cancel_token.cancel(); // Pre-cancelled

    let result = orchestrator.run_job(job, cancel_token, |_| {}).await;

    assert!(matches!(result, Err(DomainError::Cancelled)));
}
