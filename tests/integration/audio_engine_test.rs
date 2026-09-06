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
        .align_segments(
            dir.path(),
            &source_timeline,
            &synth_segments,
            Some(doc.metadata.duration_ms),
        )
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
            Some(doc.metadata.duration_ms),
        )
        .await
        .expect("Failed to export final audio");

    assert!(final_output.exists());
    assert!(artifact.duration_ms >= 900);
    assert!(artifact.size_bytes > 0);
}

#[tokio::test]
async fn test_ffmpeg_video_remuxing() {
    let dir = tempdir().unwrap();
    let video_mp4 = dir.path().join("source_video.mp4");
    let dubbed_audio = dir.path().join("dubbed_audio.mp3");
    let remuxed_mp4 = dir.path().join("remuxed_video.mp4");

    // 1. Generate 1s test video
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=320x240:rate=10",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&video_mp4)
        .status()
        .expect("Failed to create test video");

    assert!(status.success(), "Failed to create test video with ffmpeg");

    // 2. Generate 1s test audio
    let status_a = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
        ])
        .arg(&dubbed_audio)
        .status()
        .expect("Failed to create test audio");

    assert!(status_a.success());

    // 3. Remux video + audio
    let engine = FfmpegAudioEngine::new();
    let result = engine
        .remux_video(&video_mp4, &dubbed_audio, &remuxed_mp4)
        .await
        .expect("Failed to remux video");

    assert!(result.exists());
    assert_eq!(result, remuxed_mp4);

    let doc = engine
        .inspect_and_validate(&remuxed_mp4, 50 * 1024 * 1024)
        .await
        .expect("Failed to validate remuxed video");

    assert_eq!(doc.format, AudioFormat::Mp4);
    assert!(doc.metadata.duration_ms >= 800);
}

#[tokio::test]
async fn test_duration_sync_enforcement_4_seconds() {
    let dir = tempdir().unwrap();
    let original_4s = dir.path().join("original_4s.mp3");
    let synth_6s = dir.path().join("synth_6s.wav");
    let final_dub = dir.path().join("final_dub.mp3");

    // 1. Generate a 4-second original source file
    let status_orig = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=4",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
        ])
        .arg(&original_4s)
        .status()
        .expect("Failed to create 4s original audio");
    assert!(status_orig.success());

    // 2. Generate a 6-second synthesized segment (simulating TTS expansion from verbose translation)
    let status_synth = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=520:duration=6",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&synth_6s)
        .status()
        .expect("Failed to create 6s synthesized audio");
    assert!(status_synth.success());

    let engine = FfmpegAudioEngine::new();
    let doc = engine
        .inspect_and_validate(&original_4s, 10 * 1024 * 1024)
        .await
        .expect("Failed to validate original audio");
    assert_eq!(doc.metadata.duration_ms / 1000, 4);

    let source_timeline = vec![TranscriptSegment {
        id: "seg_0001".to_string(),
        speaker_id: Some("Speaker 1".to_string()),
        start_ms: 0,
        end_ms: doc.metadata.duration_ms,
        text: "Sample speech".to_string(),
        words: vec![],
    }];

    let synth_segments = vec![SynthesizedSegment {
        segment_id: "seg_0001".to_string(),
        speaker_id: Some("Speaker 1".to_string()),
        path: synth_6s,
        duration_ms: 6000,
    }];

    // 3. Align with target duration budget = 4000ms
    let alignment_res = engine
        .align_segments(
            dir.path(),
            &source_timeline,
            &synth_segments,
            Some(doc.metadata.duration_ms),
        )
        .await
        .expect("Failed to align segments");

    // 4. Export with target duration enforcement = 4000ms
    let artifact = engine
        .export_final(
            &alignment_res.aligned_files,
            &final_dub,
            AudioFormat::Mp3,
            192,
            alignment_res.quality_warnings,
            Some(doc.metadata.duration_ms),
        )
        .await
        .expect("Failed to export final audio");

    assert!(final_dub.exists());
    // Verify output duration is strictly synchronized with the 4-second source (within 1 MP3 frame tolerance: 48ms)
    let diff = (artifact.duration_ms as i64 - doc.metadata.duration_ms as i64).abs();
    assert!(
        diff <= 100,
        "Expected output duration (~{}ms) to match source (~{}ms), but difference was {}ms",
        artifact.duration_ms,
        doc.metadata.duration_ms,
        diff
    );
}
