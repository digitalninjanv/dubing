use async_trait::async_trait;
use audiodub::application::ports::{
    AudioEngine, SpeechSynthesizer, SpeechTranscriber, TextTranslator,
};
use audiodub::application::PipelineOrchestrator;
use audiodub::config::AudioConfig;
use audiodub::domain::{
    AudioDocument, DomainError, Job, LanguageId, SynthesizedSegment, Transcript, TranscriptSegment,
    TranslatedDocument, TranslationSegment, VoiceProfile, WordTimestamp,
};
use audiodub::infrastructure::ffmpeg::FfmpegAudioEngine;
use audiodub::infrastructure::filesystem::{AppPaths, FileJobRepository};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

struct CrashMockTranscriber;
#[async_trait]
impl SpeechTranscriber for CrashMockTranscriber {
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
                    text: "Bagian satu.".to_string(),
                    words: vec![WordTimestamp {
                        word: "Bagian".to_string(),
                        start_ms: 0,
                        end_ms: 500,
                    }],
                },
                TranscriptSegment {
                    id: "seg_0002".to_string(),
                    speaker_id: Some("Speaker 1".to_string()),
                    start_ms: 1000,
                    end_ms: 2000,
                    text: "Bagian dua.".to_string(),
                    words: vec![],
                },
            ],
        ))
    }
}

struct CrashMockTranslator;
#[async_trait]
impl TextTranslator for CrashMockTranslator {
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
                    source_text: "Bagian satu.".to_string(),
                    translated_text: "Part one.".to_string(),
                },
                TranslationSegment {
                    segment_id: "seg_0002".to_string(),
                    speaker_id: Some("Speaker 1".to_string()),
                    source_start_ms: 1000,
                    source_end_ms: 2000,
                    source_text: "Bagian dua.".to_string(),
                    translated_text: "Part two.".to_string(),
                },
            ],
        ))
    }
}

struct CountingSynthesizer {
    call_count: Arc<AtomicUsize>,
}
#[async_trait]
impl SpeechSynthesizer for CountingSynthesizer {
    async fn synthesize_segment(
        &self,
        segment: &TranslationSegment,
        _voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);

        let _ = Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("sine=frequency=440:duration=1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(output_path)
            .status();

        Ok(SynthesizedSegment {
            segment_id: segment.segment_id.clone(),
            speaker_id: segment.speaker_id.clone(),
            path: output_path.to_path_buf(),
            duration_ms: 1000,
        })
    }
}

#[tokio::test]
async fn test_crash_recovery_resumes_without_duplication() {
    let dir = tempdir().unwrap();
    let sample_mp3 = dir.path().join("crash_test.mp3");

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
    let job_dir = AppPaths::job_dir(job.id.as_str());
    let _ = std::fs::create_dir_all(&job_dir);

    // Pre-create seg_0001.wav as if synthesized before crash
    let existing_seg1 = job_dir.join("seg_0001.wav");
    let _ = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("sine=frequency=440:duration=1")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(&existing_seg1)
        .status();

    let synth_calls = Arc::new(AtomicUsize::new(0));
    let counting_synth = Arc::new(CountingSynthesizer {
        call_count: synth_calls.clone(),
    });

    let job_repo = Arc::new(FileJobRepository::with_dir(dir.path().join("jobs")));

    let orchestrator = PipelineOrchestrator::new(
        Arc::new(CrashMockTranscriber),
        Arc::new(CrashMockTranslator),
        counting_synth,
        engine,
        job_repo,
        AudioConfig::default(),
    );

    let cancel_token = CancellationToken::new();
    let result = orchestrator.run_job(job, cancel_token, |_| {}).await;

    assert!(result.is_ok(), "Pipeline failed: {:?}", result.err());

    // seg_0001.wav was already valid on disk, so only seg_0002 should have triggered synthesize_segment!
    assert_eq!(
        synth_calls.load(Ordering::SeqCst),
        1,
        "Expected exactly 1 synthesis call for the remaining segment, but got {}",
        synth_calls.load(Ordering::SeqCst)
    );
}
