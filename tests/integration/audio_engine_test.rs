use audiodub::application::ports::AudioEngine;
use audiodub::domain::{AudioFormat, SynthesizedSegment, TranscriptSegment};
use audiodub::infrastructure::ffmpeg::FfmpegAudioEngine;
use std::process::Command;
use tempfile::tempdir;

#[tokio::test]
async fn test_ffmpeg_probe_and_export() {
    let dir = tempdir().unwrap();
    let sample_mp3 = dir.path().join("sample.mp3");

    // Generate a valid 1-second test sine wave MP3 using ffmpeg
    let status = Command::new("ffmpeg")
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
        .expect("Failed to execute ffmpeg");

    assert!(status.success(), "ffmpeg sine wave generation failed");

    let engine = FfmpegAudioEngine::new();

    // 1. Probe & Validate
    let doc = engine
        .inspect_and_validate(&sample_mp3, 10 * 1024 * 1024)
        .await
        .expect("Failed to validate audio document");

    assert_eq!(doc.format, AudioFormat::Mp3);
    assert!(doc.metadata.duration_ms >= 900 && doc.metadata.duration_ms <= 1100);
    assert_eq!(doc.metadata.codec, "mp3");

    // 2. Alignment & Concat Export
    let source_timeline = vec![TranscriptSegment {
        id: "seg_0001".to_string(),
        speaker_id: Some("Speaker 1".to_string()),
        start_ms: 0,
        end_ms: 1000,
        text: "Testing".to_string(),
        words: vec![],
    }];

    let synth_segments = vec![SynthesizedSegment {
        segment_id: "seg_0001".to_string(),
        speaker_id: Some("Speaker 1".to_string()),
        path: sample_mp3.clone(),
        duration_ms: doc.metadata.duration_ms,
    }];

    let alignment_res = engine
        .align_segments(dir.path(), &source_timeline, &synth_segments)
        .await
        .expect("Failed to align segments");

    assert!(!alignment_res.aligned_files.is_empty());

    let final_output = dir.path().join("final_output.mp3");
    let artifact = engine
        .export_final(
            &alignment_res.aligned_files,
            &final_output,
            AudioFormat::Mp3,
            192,
            alignment_res.quality_warnings,
        )
        .await
        .expect("Failed to export final audio");

    assert!(final_output.exists());
    assert!(artifact.duration_ms >= 900);
    assert!(artifact.size_bytes > 0);
}
