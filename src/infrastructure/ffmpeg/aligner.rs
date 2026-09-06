use super::probe::FfprobeInspector;
use crate::domain::{AlignmentResult, DomainError, SynthesizedSegment, TranscriptSegment};
use std::path::Path;
use std::process::Command;

pub struct FfmpegAligner;

impl FfmpegAligner {
    /// Generates a silence audio segment (WAV) of specified duration in milliseconds and sample rate
    pub fn generate_silence(
        output_path: &Path,
        duration_ms: u64,
        sample_rate: u32,
    ) -> Result<(), DomainError> {
        let duration_secs = (duration_ms as f64) / 1000.0;
        let rate = if sample_rate == 0 { 24000 } else { sample_rate };
        let output = Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg(format!("anullsrc=r={}:cl=mono", rate))
            .arg("-t")
            .arg(format!("{:.3}", duration_secs))
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(output_path)
            .output()
            .map_err(|e| DomainError::AlignmentError(format!("Failed to run ffmpeg: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::AlignmentError(format!(
                "Failed to generate silence segment: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Trims leading and trailing silence from an audio file using the silenceremove filter
    pub fn trim_silence(input_path: &Path, output_path: &Path) -> Result<(), DomainError> {
        let output = Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(input_path)
            .arg("-af")
            .arg("silenceremove=start_periods=1:start_duration=0.03:start_threshold=-45dB:stop_periods=-1:stop_duration=0.08:stop_threshold=-45dB")
            .arg(output_path)
            .output()
            .map_err(|e| {
                DomainError::AlignmentError(format!("Failed to run ffmpeg silenceremove: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::AlignmentError(format!(
                "Failed to trim silence: {}",
                stderr
            )));
        }

        // Validate that trimmed output has a valid duration (> 100ms)
        if let Ok(meta) = FfprobeInspector::probe(output_path) {
            if meta.duration_ms > 100 {
                return Ok(());
            }
        }

        // If trimming produced empty or corrupt audio, copy input as fallback
        std::fs::copy(input_path, output_path).map_err(|e| {
            DomainError::AlignmentError(format!("Failed to fallback copy audio: {}", e))
        })?;

        Ok(())
    }

    /// Time-stretches an audio file using the atempo filter (speedup or slowdown)
    pub fn time_stretch(
        input_path: &Path,
        output_path: &Path,
        tempo: f64,
    ) -> Result<(), DomainError> {
        // Clamp tempo between 0.75 and 1.50 for natural sounding speech without distortion
        let clamped_tempo = tempo.clamp(0.75, 1.50);
        let output = Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(input_path)
            .arg("-filter:a")
            .arg(format!("atempo={:.3}", clamped_tempo))
            .arg(output_path)
            .output()
            .map_err(|e| {
                DomainError::AlignmentError(format!("Failed to run ffmpeg atempo: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::AlignmentError(format!(
                "Failed to apply atempo filter: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Aligns synthesized segments with source timeline
    pub fn align(
        job_dir: &Path,
        source_timeline: &[TranscriptSegment],
        synthesized: &[SynthesizedSegment],
        target_total_duration_ms: Option<u64>,
    ) -> Result<AlignmentResult, DomainError> {
        let mut aligned_files = Vec::new();
        let mut quality_warnings = Vec::new();
        let mut current_timeline_ms: u64 = 0;

        let sample_rate = synthesized
            .first()
            .and_then(|s| FfprobeInspector::probe(&s.path).ok())
            .map(|m| m.sample_rate)
            .unwrap_or(24000);

        for (idx, synth_seg) in synthesized.iter().enumerate() {
            // Trim leading and trailing silence from raw TTS output
            let trimmed_path = job_dir.join(format!("trimmed_{:04}.wav", idx));
            let active_path = match Self::trim_silence(&synth_seg.path, &trimmed_path) {
                Ok(()) => trimmed_path,
                Err(_) => synth_seg.path.clone(),
            };

            // Find corresponding source segment
            let source_seg = source_timeline
                .iter()
                .find(|s| s.id == synth_seg.segment_id)
                .or_else(|| source_timeline.get(idx));

            let (start_ms, end_ms) = if let Some(src) = source_seg {
                (src.start_ms, src.end_ms)
            } else {
                (
                    current_timeline_ms,
                    current_timeline_ms + synth_seg.duration_ms,
                )
            };

            // Check if there is an overlap with preceding speech
            if start_ms < current_timeline_ms && (current_timeline_ms - start_ms) > 200 {
                quality_warnings.push(format!(
                    "Segment {} overlap: target starts at {}ms but previous speech ended at {}ms",
                    synth_seg.segment_id, start_ms, current_timeline_ms
                ));
            }

            // 1. If there is a silence gap between current position and segment start, pad silence
            if start_ms > current_timeline_ms {
                let gap_ms = start_ms - current_timeline_ms;
                // Only pad if gap is noticeable (> 50ms)
                if gap_ms >= 50 {
                    let silence_path = job_dir.join(format!("silence_{:04}.wav", idx));
                    Self::generate_silence(&silence_path, gap_ms, sample_rate)?;
                    aligned_files.push(silence_path);
                    current_timeline_ms += gap_ms;
                }
            }

            // 2. Check duration and determine if time-stretching is appropriate
            let target_slot_ms = end_ms.saturating_sub(start_ms);
            let actual_metadata = FfprobeInspector::probe(&active_path)?;
            let actual_duration_ms = actual_metadata.duration_ms;

            if target_slot_ms > 0 && actual_duration_ms > (target_slot_ms + 80) {
                // Audio exceeds target slot by more than 80ms
                let ratio = (actual_duration_ms as f64) / (target_slot_ms as f64);
                if ratio <= 1.50 {
                    // Fit precisely into target slot so that 100% of the speech completes within the slot
                    let stretched_path = job_dir.join(format!("align_{:04}.wav", idx));
                    Self::time_stretch(&active_path, &stretched_path, ratio)?;
                    aligned_files.push(stretched_path);
                    current_timeline_ms += target_slot_ms;
                    continue;
                } else {
                    // Exceeds 1.50x: compress at 1.50x to preserve intelligibility without cutting words
                    let stretched_path = job_dir.join(format!("align_{:04}.wav", idx));
                    Self::time_stretch(&active_path, &stretched_path, 1.50)?;
                    aligned_files.push(stretched_path);
                    let new_duration = (actual_duration_ms as f64 / 1.50).round() as u64;
                    quality_warnings.push(format!(
                        "Segment {} duration ({}ms) exceeded target slot ({}ms) by {:.2}x; clamped time-stretch to 1.50x (new duration: {}ms)",
                        synth_seg.segment_id, actual_duration_ms, target_slot_ms, ratio, new_duration
                    ));
                    current_timeline_ms += new_duration;
                    continue;
                }
            }

            // Otherwise use the active segment directly
            aligned_files.push(active_path);
            current_timeline_ms += actual_duration_ms;
        }

        // 3. Post-timeline alignment: if total target duration is provided and current timeline is shorter, pad ending silence
        if let Some(total_ms) = target_total_duration_ms {
            if total_ms > current_timeline_ms {
                let end_gap_ms = total_ms - current_timeline_ms;
                if end_gap_ms >= 50 {
                    let end_silence_path = job_dir.join("silence_end.wav");
                    Self::generate_silence(&end_silence_path, end_gap_ms, sample_rate)?;
                    aligned_files.push(end_silence_path);
                }
            }
        }

        Ok(AlignmentResult {
            aligned_files,
            quality_warnings,
        })
    }
}
