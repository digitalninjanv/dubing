use super::probe::FfprobeInspector;
use crate::domain::{AudioArtifact, AudioFormat, DomainError};
use std::fs::File;
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
    ) -> Result<AudioArtifact, DomainError> {
        if segments.is_empty() {
            return Err(DomainError::ExportError(
                "No audio segments to export".to_string(),
            ));
        }

        let parent_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
        let _ = std::fs::create_dir_all(parent_dir);
        let concat_list_path = parent_dir.join("concat_list.txt");

        // Write concat list file
        let mut list_file = File::create(&concat_list_path).map_err(|e| {
            DomainError::ExportError(format!("Failed to create concat list: {}", e))
        })?;

        for seg in segments {
            // Use absolute canonicalized path or plain string with safe escaping
            let path_str = seg.to_string_lossy().replace('\'', "'\\''");
            writeln!(list_file, "file '{}'", path_str).map_err(|e| {
                DomainError::ExportError(format!("Failed to write concat list: {}", e))
            })?;
        }
        drop(list_file);

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")
            .arg("-i")
            .arg(&concat_list_path);

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

        // Cleanup concat list
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
        })
    }
}
