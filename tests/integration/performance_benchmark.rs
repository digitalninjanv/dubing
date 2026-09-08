use audiodub::application::ports::AudioEngine;
use audiodub::domain::{SynthesizedSegment, TranscriptSegment};
use audiodub::infrastructure::ffmpeg::FfmpegAudioEngine;
use std::process::Command;
use std::time::Instant;
use tempfile::tempdir;

/// Generates a test WAV tone of specified duration
fn generate_tone(path: &std::path::Path, duration_secs: f64) {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={:.2}", duration_secs),
            "-c:a",
            "pcm_s16le",
            "-ar",
            "24000",
        ])
        .arg(path)
        .output()
        .expect("Failed to execute ffmpeg");
    assert!(status.status.success());
}

#[tokio::test]
async fn benchmark_ffmpeg_alignment_ten_segments() {
    let temp = tempdir().expect("Failed to create tempdir");
    let job_dir = temp.path().join("job_bench");
    std::fs::create_dir_all(&job_dir).unwrap();

    let mut source_timeline = Vec::new();
    let mut synth_segments = Vec::new();

    // Create 10 segments: each source slot is 1.5s (1500ms), synth audio is 1.8s (exceeds slot by 300ms, requires stretch)
    for i in 0..10 {
        let seg_id = format!("seg_{:04}", i + 1);
        let start_ms = i * 2000;
        let end_ms = start_ms + 1500;

        let tone_path = job_dir.join(format!("raw_synth_{:04}.wav", i + 1));
        generate_tone(&tone_path, 1.8);

        source_timeline.push(TranscriptSegment {
            id: seg_id.clone(),
            speaker_id: Some("Speaker 1".to_string()),
            start_ms,
            end_ms,
            text: format!("Segment number {}", i + 1),
            words: Vec::new(),
        });

        synth_segments.push(SynthesizedSegment {
            segment_id: seg_id,
            speaker_id: Some("Speaker 1".to_string()),
            path: tone_path,
            duration_ms: 1800,
        });
    }

    let engine = FfmpegAudioEngine::new();
    let start = Instant::now();
    let res = engine
        .align_segments(&job_dir, &source_timeline, &synth_segments, Some(20000))
        .await
        .expect("Alignment failed");
    let elapsed = start.elapsed();

    println!(
        "\n[BENCHMARK] FFmpeg Alignment of 10 Segments took: {:.2?} ({} files generated)",
        elapsed,
        res.aligned_files.len()
    );
    assert!(!res.aligned_files.is_empty());
}

use async_trait::async_trait;
use audiodub::application::ports::{SpeechSynthesizer, SpeechTranscriber, TextTranslator};
use audiodub::application::PipelineOrchestrator;
use audiodub::domain::{
    AudioDocument, DomainError, Job, LanguageId, Transcript, TranslatedDocument,
    TranslationSegment, VoiceProfile,
};
use audiodub::infrastructure::filesystem::FileJobRepository;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct BenchTranscriber(Vec<TranscriptSegment>);
#[async_trait]
impl SpeechTranscriber for BenchTranscriber {
    async fn transcribe(
        &self,
        _audio: &AudioDocument,
        _hint: &LanguageId,
    ) -> Result<Transcript, DomainError> {
        Ok(Transcript::new(LanguageId::new("id"), self.0.clone()))
    }
}

struct BenchTranslator(Vec<TranslationSegment>);
#[async_trait]
impl TextTranslator for BenchTranslator {
    async fn translate(
        &self,
        _transcript: &Transcript,
        target_lang: &LanguageId,
        _tone: audiodub::domain::TranslationTone,
    ) -> Result<TranslatedDocument, DomainError> {
        Ok(TranslatedDocument::new(
            LanguageId::new("id"),
            target_lang.clone(),
            self.0.clone(),
        ))
    }
}

struct LatencyMockSynthesizer {
    latency_ms: u64,
}
#[async_trait]
impl SpeechSynthesizer for LatencyMockSynthesizer {
    async fn synthesize_segment(
        &self,
        segment: &TranslationSegment,
        _voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
        // Generate a real lightweight 1-second WAV
        let _ = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "pcm_s16le",
                "-ar",
                "24000",
            ])
            .arg(output_path)
            .output();

        Ok(SynthesizedSegment {
            segment_id: segment.segment_id.clone(),
            speaker_id: segment.speaker_id.clone(),
            path: output_path.to_path_buf(),
            duration_ms: 1000,
        })
    }
}

#[tokio::test]
async fn benchmark_pipeline_concurrent_synthesis_ten_segments() {
    let dir = tempdir().unwrap();
    let sample_mp3 = dir.path().join("input_bench.mp3");

    // Generate real 20-second audio
    let _ = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=20",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
        ])
        .arg(&sample_mp3)
        .output()
        .expect("ffmpeg failed");

    let mut transcript_segs = Vec::new();
    let mut translation_segs = Vec::new();

    for i in 0..10 {
        let seg_id = format!("seg_{:04}", i + 1);
        let start_ms = i * 2000;
        let end_ms = start_ms + 1500;
        transcript_segs.push(TranscriptSegment {
            id: seg_id.clone(),
            speaker_id: Some("Speaker 1".to_string()),
            start_ms,
            end_ms,
            text: format!("Kalimat {}", i + 1),
            words: Vec::new(),
        });
        translation_segs.push(TranslationSegment {
            segment_id: seg_id,
            speaker_id: Some("Speaker 1".to_string()),
            source_start_ms: start_ms,
            source_end_ms: end_ms,
            source_text: format!("Kalimat {}", i + 1),
            translated_text: format!("Sentence {}", i + 1),
        });
    }

    let transcriber = Arc::new(BenchTranscriber(transcript_segs));
    let translator = Arc::new(BenchTranslator(translation_segs));
    // 250ms latency per segment
    let synthesizer = Arc::new(LatencyMockSynthesizer { latency_ms: 250 });
    let audio_engine = Arc::new(FfmpegAudioEngine::new());
    let repo_dir = dir.path().join("repo");
    let job_repo = Arc::new(FileJobRepository::with_dir(repo_dir));

    let doc = audio_engine
        .inspect_and_validate(&sample_mp3, 100 * 1024 * 1024)
        .await
        .expect("Inspection failed");

    let orchestrator = PipelineOrchestrator::new(
        transcriber,
        translator,
        synthesizer,
        audio_engine,
        job_repo,
        audiodub::config::AudioConfig::default(),
    );

    let job = Job::new(doc, LanguageId::new("id"), LanguageId::new("en"));
    let cancel = CancellationToken::new();

    let bench_start = Instant::now();
    let artifact = orchestrator
        .run_job(job, cancel, |_| {})
        .await
        .expect("Job execution failed");
    let total_elapsed = bench_start.elapsed();

    println!(
        "\n[BENCHMARK] Full Pipeline 10 Segments (Concurrent TTS + Parallel Alignment): took {:.2?} (Output duration: {}ms)",
        total_elapsed,
        artifact.duration_ms
    );
    assert!(artifact.duration_ms > 0);
}
