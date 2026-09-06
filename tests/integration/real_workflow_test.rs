use async_trait::async_trait;
use audiodub::application::ports::{
    AudioEngine, JobRepository, SpeechSynthesizer, SpeechTranscriber, TextTranslator,
};
use audiodub::application::PipelineOrchestrator;
use audiodub::config::AudioConfig;
use audiodub::domain::{
    AudioDocument, AudioFormat, DomainError, Job, LanguageId, PipelineStage, SynthesizedSegment,
    Transcript, TranscriptSegment, TranslatedDocument, TranslationSegment, VoiceProfile,
    WordTimestamp,
};
use audiodub::infrastructure::ffmpeg::FfmpegAudioEngine;
use audiodub::infrastructure::filesystem::FileJobRepository;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

struct RealWorkflowTranscriber;
#[async_trait]
impl SpeechTranscriber for RealWorkflowTranscriber {
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
                    text: "Halo, selamat datang di podcast.".to_string(),
                    words: vec![
                        WordTimestamp {
                            word: "Halo".to_string(),
                            start_ms: 0,
                            end_ms: 300,
                        },
                        WordTimestamp {
                            word: "selamat".to_string(),
                            start_ms: 350,
                            end_ms: 600,
                        },
                    ],
                },
                TranscriptSegment {
                    id: "seg_0002".to_string(),
                    speaker_id: Some("Speaker 2".to_string()),
                    start_ms: 1200,
                    end_ms: 2200,
                    text: "Terima kasih, senang bisa hadir di sini.".to_string(),
                    words: vec![],
                },
                TranscriptSegment {
                    id: "seg_0003".to_string(),
                    speaker_id: Some("Speaker 1".to_string()),
                    start_ms: 2500,
                    end_ms: 3500,
                    text: "Mari kita mulai topik utama hari ini.".to_string(),
                    words: vec![],
                },
            ],
        ))
    }
}

struct RealWorkflowTranslator;
#[async_trait]
impl TextTranslator for RealWorkflowTranslator {
    async fn translate(
        &self,
        _transcript: &Transcript,
        target_lang: &LanguageId,
        _tone: audiodub::domain::TranslationTone,
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
                    source_text: "Halo, selamat datang di podcast.".to_string(),
                    translated_text: "Hello, welcome to the podcast.".to_string(),
                },
                TranslationSegment {
                    segment_id: "seg_0002".to_string(),
                    speaker_id: Some("Speaker 2".to_string()),
                    source_start_ms: 1200,
                    source_end_ms: 2200,
                    source_text: "Terima kasih, senang bisa hadir di sini.".to_string(),
                    translated_text: "Thank you, glad to be here.".to_string(),
                },
                TranslationSegment {
                    segment_id: "seg_0003".to_string(),
                    speaker_id: Some("Speaker 1".to_string()),
                    source_start_ms: 2500,
                    source_end_ms: 3500,
                    source_text: "Mari kita mulai topik utama hari ini.".to_string(),
                    translated_text: "Let's begin today's main discussion.".to_string(),
                },
            ],
        ))
    }
}

struct RealWorkflowSynthesizer;
#[async_trait]
impl SpeechSynthesizer for RealWorkflowSynthesizer {
    async fn synthesize_segment(
        &self,
        segment: &TranslationSegment,
        voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        let freq = if voice.voice_name == "Aoede" {
            440
        } else if voice.voice_name == "Puck" {
            520
        } else {
            480
        };

        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg(format!("sine=frequency={}:duration=1", freq))
            .arg("-ar")
            .arg("24000")
            .arg("-ac")
            .arg("1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(output_path)
            .status()
            .map_err(|e| DomainError::Internal(format!("ffmpeg failed: {}", e)))?;

        assert!(
            status.success(),
            "Failed to generate synthesized audio segment"
        );

        Ok(SynthesizedSegment {
            segment_id: segment.segment_id.clone(),
            speaker_id: segment.speaker_id.clone(),
            path: output_path.to_path_buf(),
            duration_ms: 1000,
        })
    }
}

#[tokio::test]
async fn test_real_end_to_end_mp3_workflow() {
    let dir = tempdir().unwrap();
    let input_mp3 = dir.path().join("podcast_input.mp3");

    // 1. Generate real 3.5s multi-tone input audio simulating spoken podcast
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("sine=frequency=440:duration=3.5")
        .arg("-c:a")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg("192k")
        .arg(&input_mp3)
        .status()
        .expect("Failed to run ffmpeg for test input");
    assert!(status.success(), "Input MP3 generation failed");

    // 2. Audio Engine inspection & validation
    let engine = Arc::new(FfmpegAudioEngine::new());
    let doc = engine
        .inspect_and_validate(&input_mp3, 100 * 1024 * 1024)
        .await
        .expect("Audio inspection failed");

    assert_eq!(doc.format, AudioFormat::Mp3);
    assert_eq!(doc.metadata.codec, "mp3");
    assert!(doc.metadata.duration_ms >= 3400 && doc.metadata.duration_ms <= 3600);
    assert!(doc.size_bytes > 0);

    // 3. Create job and orchestrator
    let job = Job::new(doc, LanguageId::auto(), LanguageId::new("en"));
    let job_id = job.id.clone();
    let job_repo_dir = dir.path().join("jobs");
    let job_repo = Arc::new(FileJobRepository::with_dir(job_repo_dir));

    let audio_config = AudioConfig {
        max_file_size_bytes: 100 * 1024 * 1024,
        default_bitrate_kbps: 192,
        export_wav: false,
    };

    let orchestrator = PipelineOrchestrator::new(
        Arc::new(RealWorkflowTranscriber),
        Arc::new(RealWorkflowTranslator),
        Arc::new(RealWorkflowSynthesizer),
        engine.clone(),
        job_repo.clone(),
        audio_config,
    );

    let cancel_token = CancellationToken::new();
    let progress_events = Arc::new(AtomicUsize::new(0));
    let progress_events_clone = progress_events.clone();

    // 4. Execute pipeline
    let result = orchestrator
        .run_job(job, cancel_token, move |updated_job| {
            progress_events_clone.fetch_add(1, Ordering::SeqCst);
            println!(
                "Progress [{:.0}%] stage: {:?}, msg: {}",
                updated_job.progress.fraction() * 100.0,
                updated_job.stage,
                updated_job.progress.message
            );
        })
        .await;

    assert!(result.is_ok(), "Pipeline failed: {:?}", result.err());
    let artifact = result.unwrap();

    // 5. Verify exported audio file
    assert_eq!(artifact.format, AudioFormat::Mp3);
    assert!(
        artifact.path.exists(),
        "Output MP3 does not exist: {}",
        artifact.path.display()
    );
    assert!(artifact.size_bytes > 0, "Output MP3 file is empty");
    assert!(
        artifact.duration_ms >= 3400,
        "Output duration is too short: {}ms",
        artifact.duration_ms
    );

    // 6. Detailed probe verification of the output MP3 using Ffprobe
    let output_meta = engine
        .probe(&artifact.path)
        .await
        .expect("Failed to probe final output MP3");

    assert_eq!(output_meta.codec, "mp3");
    assert!(output_meta.duration_ms >= 3400);
    assert_eq!(output_meta.channels, 1);

    // 7. Verify Job Repository persistence
    let saved_job = job_repo
        .load(&job_id)
        .await
        .expect("Failed to load saved job from repository")
        .expect("Saved job not found in repository");

    assert_eq!(saved_job.stage, PipelineStage::Completed);
    assert_eq!(saved_job.progress.completed_segments, 3);
    assert_eq!(saved_job.progress.total_segments, 3);
    assert!(progress_events.load(Ordering::SeqCst) >= 8);
}
