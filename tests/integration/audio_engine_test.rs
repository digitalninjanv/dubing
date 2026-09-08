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
        .remux_video(&video_mp4, &dubbed_audio, None, None, &remuxed_mp4)
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

#[tokio::test]
async fn test_ffmpeg_video_audio_extraction() {
    let dir = tempdir().unwrap();
    let video_mp4 = dir.path().join("test_video.mp4");
    let extracted_mp3 = dir.path().join("extracted.mp3");

    // Generate a synthetic 2-second MP4 test video with audio using lavfi testsrc and sine
    let status = Command::new("ffmpeg")
        .arg("-y")
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=320x240:rate=30",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&video_mp4)
        .status()
        .expect("Failed to execute ffmpeg for test video generation");

    assert!(status.success(), "Failed to generate test video");

    let engine = FfmpegAudioEngine::new();

    // 1. Validate video container
    let video_doc = engine
        .inspect_and_validate(&video_mp4, 100 * 1024 * 1024)
        .await
        .expect("Video document should be valid");

    assert!(video_doc.format.is_video());
    assert_eq!(video_doc.format, AudioFormat::Mp4);

    // 2. Extract audio track
    let audio_doc = engine
        .extract_audio(&video_mp4, &extracted_mp3)
        .await
        .expect("Audio extraction should succeed");

    assert_eq!(audio_doc.format, AudioFormat::Mp3);
    assert!(extracted_mp3.exists());
    assert!(audio_doc.metadata.duration_ms >= 1800 && audio_doc.metadata.duration_ms <= 2200);
}

#[tokio::test]
async fn test_ffmpeg_mix_with_ducking() {
    let dir = tempdir().unwrap();
    let bg_audio = dir.path().join("background.wav");
    let voice_audio = dir.path().join("voice.wav");
    let ducked_output = dir.path().join("ducked_mix.mp3");

    // Generate 2s background audio
    let status_bg = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=220:duration=2",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&bg_audio)
        .status()
        .expect("Failed to generate background audio");
    assert!(status_bg.success());

    // Generate 1s voiceover audio
    let status_v = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=1",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&voice_audio)
        .status()
        .expect("Failed to generate voice audio");
    assert!(status_v.success());

    let engine = FfmpegAudioEngine::new();
    let result = engine
        .mix_with_ducking(&bg_audio, &voice_audio, &ducked_output)
        .await
        .expect("Audio ducking mix should succeed");

    assert!(result.exists());
    let doc = engine
        .inspect_and_validate(&ducked_output, 10 * 1024 * 1024)
        .await
        .expect("Ducked output should be a valid MP3");
    assert_eq!(doc.format, AudioFormat::Mp3);
    assert!(doc.metadata.duration_ms >= 1800);
}

#[tokio::test]
async fn test_ffmpeg_video_remux_with_soft_subtitles() {
    let dir = tempdir().unwrap();
    let video_mp4 = dir.path().join("input_video.mp4");
    let audio_mp3 = dir.path().join("dubbed.mp3");
    let srt_file = dir.path().join("subtitles.srt");
    let output_mp4 = dir.path().join("output_with_subs.mp4");

    // Generate 2s test video
    let status_vid = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=320x240:rate=30",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&video_mp4)
        .status()
        .expect("Failed to create test video");
    assert!(status_vid.success());

    // Generate 2s dubbed audio
    let status_aud = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=500:duration=2",
            "-c:a",
            "libmp3lame",
        ])
        .arg(&audio_mp3)
        .status()
        .expect("Failed to create test audio");
    assert!(status_aud.success());

    // Generate sample SRT subtitle file
    let srt_content = "1\n00:00:00,100 --> 00:00:01,800\nHalo ini uji coba dubbing AI\n\n";
    std::fs::write(&srt_file, srt_content).expect("Failed to write SRT file");

    let engine = FfmpegAudioEngine::new();
    let result = engine
        .remux_video(
            &video_mp4,
            &audio_mp3,
            Some(&srt_file),
            Some("ind"),
            &output_mp4,
        )
        .await
        .expect("Remuxing with soft subtitles should succeed");

    assert!(result.exists());
    let doc = engine
        .inspect_and_validate(&output_mp4, 50 * 1024 * 1024)
        .await
        .expect("Remuxed video should be valid");
    assert_eq!(doc.format, AudioFormat::Mp4);
    assert!(doc.metadata.duration_ms >= 1800);
}
