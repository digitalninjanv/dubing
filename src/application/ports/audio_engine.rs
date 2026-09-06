use crate::domain::{
    AlignmentResult, AudioArtifact, AudioDocument, AudioFormat, DomainError, MediaMetadata,
    SynthesizedSegment, TranscriptSegment,
};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

#[async_trait]
pub trait AudioEngine: Send + Sync {
    /// Inspect an audio file using ffprobe
    async fn probe(&self, path: &Path) -> Result<MediaMetadata, DomainError>;

    /// Create an AudioDocument after validation
    async fn inspect_and_validate(
        &self,
        path: &Path,
        max_bytes: u64,
    ) -> Result<AudioDocument, DomainError>;

    /// Align synthesized segments to the source timeline (silence padding & slight time stretching)
    async fn align_segments(
        &self,
        job_dir: &Path,
        source_timeline: &[TranscriptSegment],
        synthesized: &[SynthesizedSegment],
        target_total_duration_ms: Option<u64>,
    ) -> Result<AlignmentResult, DomainError>;

    /// Export aligned segments to the final output file (MP3 or WAV)
    async fn export_final(
        &self,
        aligned_segments: &[PathBuf],
        output_path: &Path,
        format: AudioFormat,
        bitrate_kbps: u32,
        quality_warnings: Vec<String>,
        target_duration_ms: Option<u64>,
    ) -> Result<AudioArtifact, DomainError>;

    /// Remux video with new audio track
    async fn remux_video(
        &self,
        video_input: &Path,
        audio_input: &Path,
        output_video: &Path,
    ) -> Result<PathBuf, DomainError>;
}
