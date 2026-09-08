use super::aligner::FfmpegAligner;
use super::exporter::FfmpegExporter;
use super::probe::FfprobeInspector;
use crate::application::ports::AudioEngine;
use crate::domain::{
    AlignmentResult, AudioArtifact, AudioDocument, AudioFormat, DomainError, MediaMetadata,
    SynthesizedSegment, TranscriptSegment,
};
use async_trait::async_trait;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct FfmpegAudioEngine;

impl FfmpegAudioEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FfmpegAudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AudioEngine for FfmpegAudioEngine {
    async fn probe(&self, path: &Path) -> Result<MediaMetadata, DomainError> {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || FfprobeInspector::probe(&p))
            .await
            .map_err(|e| DomainError::Internal(format!("Task join error: {}", e)))?
    }

    async fn inspect_and_validate(
        &self,
        path: &Path,
        max_bytes: u64,
    ) -> Result<AudioDocument, DomainError> {
        if !path.exists() {
            return Err(DomainError::InvalidAudio(format!(
                "File does not exist: {}",
                path.display()
            )));
        }

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let format = AudioFormat::from_extension(&extension).ok_or_else(|| {
            DomainError::UnsupportedFormat(format!(
                "Audio extension '.{}' is not supported. Supported: mp3, wav, m4a, aac, ogg, flac, webm, opus",
                extension
            ))
        })?;

        let size_bytes = fs::metadata(path)
            .map(|m| m.len())
            .map_err(|e| DomainError::InvalidAudio(format!("Failed to read file size: {}", e)))?;

        let metadata = self.probe(path).await?;

        let doc = AudioDocument {
            id: format!("doc_{}", Uuid::new_v4().simple()),
            path: path.to_path_buf(),
            format,
            mime_type: format.mime_type().to_string(),
            size_bytes,
            metadata,
        };

        doc.validate_for_processing(max_bytes)
            .map_err(DomainError::InvalidAudio)?;

        Ok(doc)
    }

    async fn align_segments(
        &self,
        job_dir: &Path,
        source_timeline: &[TranscriptSegment],
        synthesized: &[SynthesizedSegment],
        target_total_duration_ms: Option<u64>,
    ) -> Result<AlignmentResult, DomainError> {
        let job_dir_owned = job_dir.to_path_buf();
        let source_timeline_owned = source_timeline.to_vec();
        let synthesized_owned = synthesized.to_vec();

        tokio::task::spawn_blocking(move || {
            FfmpegAligner::align(
                &job_dir_owned,
                &source_timeline_owned,
                &synthesized_owned,
                target_total_duration_ms,
            )
        })
        .await
        .map_err(|e| DomainError::Internal(format!("Task join error: {}", e)))?
    }

    async fn export_final(
        &self,
        aligned_segments: &[PathBuf],
        output_path: &Path,
        format: AudioFormat,
        bitrate_kbps: u32,
        quality_warnings: Vec<String>,
        target_duration_ms: Option<u64>,
    ) -> Result<AudioArtifact, DomainError> {
        let segments_owned = aligned_segments.to_vec();
        let output_path_owned = output_path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            FfmpegExporter::export(
                &segments_owned,
                &output_path_owned,
                format,
                bitrate_kbps,
                quality_warnings,
                target_duration_ms,
            )
        })
        .await
        .map_err(|e| DomainError::Internal(format!("Task join error: {}", e)))?
    }

    async fn remux_video(
        &self,
        video_input: &Path,
        audio_input: &Path,
        output_video: &Path,
    ) -> Result<PathBuf, DomainError> {
        let video_in = video_input.to_path_buf();
        let audio_in = audio_input.to_path_buf();
        let video_out = output_video.to_path_buf();

        tokio::task::spawn_blocking(move || {
            FfmpegExporter::remux_video(&video_in, &audio_in, &video_out)
        })
        .await
        .map_err(|e| DomainError::Internal(format!("Task join error: {}", e)))?
    }

    async fn extract_audio(
        &self,
        video_path: &Path,
        output_audio_path: &Path,
    ) -> Result<AudioDocument, DomainError> {
        let v_in = video_path.to_path_buf();
        let a_out = output_audio_path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            FfmpegExporter::extract_audio(&v_in, &a_out)
        })
        .await
        .map_err(|e| DomainError::Internal(format!("Task join error: {}", e)))??;

        self.inspect_and_validate(output_audio_path, 500 * 1024 * 1024)
            .await
    }
}
