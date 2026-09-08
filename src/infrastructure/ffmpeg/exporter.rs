use super::probe::FfprobeInspector;
use crate::domain::{AudioArtifact, AudioFormat, DomainError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct FfmpegExporter;

impl FfmpegExporter {
    pub fn export(
        segments: &[PathBuf],
        output_path: &Path,
        format: AudioFormat,
        bitrate_kbps: u32,
        quality_warnings: Vec<String>,
        target_duration_ms: Option<u64>,
    ) -> Result<AudioArtifact, DomainError> {
        if segments.is_empty() {
            return Err(DomainError::ExportError(
                "No audio segments to export".to_string(),
            ));
        }

        let parent_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
        let _ = std::fs::create_dir_all(parent_dir);
        // F12: unique temp file per export to avoid races when two jobs
        // export to the same parent dir concurrently.
        let mut concat_list_file = tempfile::Builder::new()
            .prefix("audiodub_concat_")
            .suffix(".txt")
            .tempfile_in(parent_dir)
            .map_err(|e| {
                DomainError::ExportError(format!("Failed to create concat list: {}", e))
            })?;
        let concat_list_path = concat_list_file.path().to_path_buf();

        for seg in segments {
            // Prefer canonical absolute paths for concat demuxer stability;
            // fall back to the raw path string with quote escaping.
            let abs = std::fs::canonicalize(seg)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| seg.to_string_lossy().into_owned());
            let path_str = abs.replace('\'', "'\\''");
            writeln!(concat_list_file, "file '{}'", path_str).map_err(|e| {
                DomainError::ExportError(format!("Failed to write concat list: {}", e))
            })?;
        }
        // Flush so ffmpeg sees the content; keep NamedTempFile alive until after ffmpeg.
        concat_list_file
            .flush()
            .map_err(|e| DomainError::ExportError(format!("Failed to flush concat list: {}", e)))?;

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .arg("-i")
            .arg(&concat_list_path);

        // Master Duration Synchronization:
        // If target_duration_ms is provided, pad silence up to minimum duration if audio is shorter.
        // We use apad=whole_dur to guarantee speech is NEVER amputated or cut off mid-sentence.
        if let Some(target_ms) = target_duration_ms {
            if target_ms > 0 {
                let duration_secs = (target_ms as f64) / 1000.0;
                cmd.arg("-af")
                    .arg(format!("apad=whole_dur={:.3}", duration_secs));
            }
        }

        match format {
            AudioFormat::Mp3 => {
                cmd.arg("-c:a")
                    .arg("libmp3lame")
                    .arg("-b:a")
                    .arg(format!("{}k", bitrate_kbps));
            }
            AudioFormat::Wav => {
                cmd.arg("-c:a").arg("pcm_s16le");
            }
            _ => {
                cmd.arg("-c:a").arg("libmp3lame").arg("-b:a").arg("192k");
            }
        }

        cmd.arg(output_path);

        let output = cmd.output().map_err(|e| {
            DomainError::ExportError(format!("Failed to execute ffmpeg concat: {}", e))
        })?;

        // Cleanup concat list (unique tempfile — remove by path)
        let _ = std::fs::remove_file(&concat_list_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::ExportError(format!(
                "ffmpeg concat failed: {}",
                stderr
            )));
        }

        // Validate final output using ffprobe (Quality Gate)
        let metadata = FfprobeInspector::probe(output_path)?;
        if metadata.duration_ms == 0 {
            return Err(DomainError::ExportError(
                "Exported audio file has 0 duration".to_string(),
            ));
        }

        let file_size = std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);

        if file_size == 0 {
            return Err(DomainError::ExportError(
                "Exported audio file is empty (0 bytes)".to_string(),
            ));
        }

        Ok(AudioArtifact {
            path: output_path.to_path_buf(),
            format,
            duration_ms: metadata.duration_ms,
            size_bytes: file_size,
            quality_warnings,
            subtitle_srt_path: None,
            subtitle_vtt_path: None,
            transcript_txt_path: None,
            video_path: None,
        })
    }

    /// Remux original video stream with new dubbed audio track and optional embedded soft subtitles
    pub fn remux_video(
        video_input: &Path,
        audio_input: &Path,
        subtitle_input: Option<&Path>,
        subtitle_language: Option<&str>,
        output_video: &Path,
    ) -> Result<PathBuf, DomainError> {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-i"])
            .arg(video_input)
            .arg("-i")
            .arg(audio_input);

        let has_subtitles = subtitle_input.map(|p| p.exists()).unwrap_or(false);
        if let Some(sub_path) = subtitle_input {
            if has_subtitles {
                cmd.arg("-i").arg(sub_path);
            }
        }

        cmd.args([
            "-c:v", "copy", "-c:a", "aac", "-map", "0:v:0", "-map", "1:a:0",
        ]);

        if has_subtitles {
            cmd.args(["-map", "2:s:0"]);
            let ext = output_video
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("mp4")
                .to_lowercase();

            if ext == "mkv" || ext == "webm" {
                cmd.args(["-c:s", "srt"]);
            } else {
                // MP4 / MOV standard closed captions
                cmd.args(["-c:s", "mov_text"]);
            }

            let lang = subtitle_language.unwrap_or("und");
            cmd.args([
                "-metadata:s:s:0",
                &format!("language={}", lang),
                "-metadata:s:s:0",
                "title=Dubbed Subtitles",
            ]);
        }

        cmd.arg("-shortest").arg(output_video);

        let output = cmd.output().map_err(|e| {
            DomainError::ExportError(format!("Failed to execute ffmpeg remux: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::ExportError(format!(
                "FFmpeg remux failed with exit code {:?}: {}",
                output.status.code(),
                stderr
            )));
        }

        if !output_video.exists() {
            return Err(DomainError::ExportError(
                "Remuxed video file was not created".to_string(),
            ));
        }

        Ok(output_video.to_path_buf())
    }

    /// Dynamically ducks background audio under a voiceover track using FFmpeg sidechaincompress
    pub fn mix_with_ducking(
        background_audio: &Path,
        voiceover_audio: &Path,
        output_audio: &Path,
    ) -> Result<PathBuf, DomainError> {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-i"])
            .arg(background_audio)
            .arg("-i")
            .arg(voiceover_audio)
            .args([
                "-filter_complex",
                "[1:a]apad[sc_padded];[sc_padded]asplit=2[sc][voice];[0:a][sc]sidechaincompress=threshold=0.08:ratio=5:attack=40:release=350[bg];[bg][voice]amix=inputs=2:duration=first:dropout_transition=2[out]",
                "-map",
                "[out]",
            ]);

        let ext = output_audio
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("mp3")
            .to_lowercase();

        if ext == "mp3" {
            cmd.args(["-c:a", "libmp3lame", "-b:a", "192k"]);
        } else if ext == "wav" {
            cmd.args(["-c:a", "pcm_s16le"]);
        } else {
            cmd.args(["-c:a", "aac", "-b:a", "192k"]);
        }

        cmd.arg(output_audio);

        let output = cmd.output().map_err(|e| {
            DomainError::ExportError(format!("Failed to execute ffmpeg audio ducking: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::ExportError(format!(
                "FFmpeg audio ducking failed with exit code {:?}: {}",
                output.status.code(),
                stderr
            )));
        }

        if !output_audio.exists() {
            return Err(DomainError::ExportError(
                "Ducked audio file was not created".to_string(),
            ));
        }

        Ok(output_audio.to_path_buf())
    }

    /// Extract audio track from video into an optimized, lightweight MP3 audio file for STT/AI ingestion
    pub fn extract_audio(video_input: &Path, output_audio: &Path) -> Result<PathBuf, DomainError> {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-i"])
            .arg(video_input)
            .args([
                "-vn",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "128k",
                "-ar",
                "24000",
                "-ac",
                "1",
            ])
            .arg(output_audio);

        let output = cmd.output().map_err(|e| {
            DomainError::ExportError(format!("Failed to execute ffmpeg audio extraction: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::ExportError(format!(
                "FFmpeg audio extraction failed with exit code {:?}: {}",
                output.status.code(),
                stderr
            )));
        }

        if !output_audio.exists() {
            return Err(DomainError::ExportError(
                "Extracted audio file was not created".to_string(),
            ));
        }

        Ok(output_audio.to_path_buf())
    }
}
