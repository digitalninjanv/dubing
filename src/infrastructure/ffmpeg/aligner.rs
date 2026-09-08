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

    /// Single-pass audio filter: trims silence and applies time-stretching simultaneously
    pub fn process_segment_single_pass(
        input_path: &Path,
        output_path: &Path,
        tempo: Option<f64>,
    ) -> Result<u64, DomainError> {
        let filter = if let Some(t) = tempo {
            let clamped = t.clamp(0.75, 1.50);
            format!("silenceremove=start_periods=1:start_duration=0.03:start_threshold=-45dB:stop_periods=-1:stop_duration=0.08:stop_threshold=-45dB,atempo={:.3}", clamped)
        } else {
            "silenceremove=start_periods=1:start_duration=0.03:start_threshold=-45dB:stop_periods=-1:stop_duration=0.08:stop_threshold=-45dB".to_string()
        };

        let output = Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(input_path)
            .arg("-af")
            .arg(&filter)
            .arg(output_path)
            .output()
            .map_err(|e| DomainError::AlignmentError(format!("Failed to run ffmpeg: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::AlignmentError(format!(
                "ffmpeg filter failed: {}",
                stderr
            )));
        }

        if let Ok(meta) = FfprobeInspector::probe(output_path) {
            if meta.duration_ms > 100 {
                return Ok(meta.duration_ms);
            }
        }

        // Fallback copy if output is empty or probe failed
        let _ = std::fs::copy(input_path, output_path);
        let meta = FfprobeInspector::probe(output_path)?;
        Ok(meta.duration_ms)
    }

    /// Aligns synthesized segments with source timeline using parallel single-pass processing
    pub fn align(
        job_dir: &Path,
        source_timeline: &[TranscriptSegment],
        synthesized: &[SynthesizedSegment],
        target_total_duration_ms: Option<u64>,
    ) -> Result<AlignmentResult, DomainError> {
        // Reuse known duration/sample_rate from SynthesizedSegment where
        // possible; probing is fallback only (F4: dedup probe).
        // SynthesizedSegment.duration_ms is already probed at TTS time.
        let sample_rate = 24000u32;

        // 1. Prepare segment plans
        struct SegmentPlan {
            synth_path: std::path::PathBuf,
            target_path: std::path::PathBuf,
            target_slot_ms: u64,
            raw_duration_ms: u64,
            segment_id: String,
            start_ms: u64,
        }

        let mut plans = Vec::with_capacity(synthesized.len());
        for (idx, synth_seg) in synthesized.iter().enumerate() {
            let target_path = job_dir.join(format!("align_{:04}.wav", idx));
            let source_seg = source_timeline
                .iter()
                .find(|s| s.id == synth_seg.segment_id)
                .or_else(|| source_timeline.get(idx));

            let (start_ms, end_ms) = if let Some(src) = source_seg {
                (src.start_ms, src.end_ms)
            } else {
                (0, synth_seg.duration_ms)
            };
            let target_slot_ms = end_ms.saturating_sub(start_ms);

            plans.push(SegmentPlan {
                synth_path: synth_seg.path.clone(),
                target_path,
                target_slot_ms,
                raw_duration_ms: synth_seg.duration_ms,
                segment_id: synth_seg.segment_id.clone(),
                start_ms,
            });
        }

        // 2. Parallel processing with bounded concurrency (F3: avoid 100
        // threads + 100 ffmpeg processes on large jobs).
        struct ProcessedSegment {
            path: std::path::PathBuf,
            duration_ms: u64,
            warning: Option<String>,
            start_ms: u64,
            segment_id: String,
        }

        let max_parallel = std::thread::available_parallelism()
            .map(|n| (n.get() * 2).clamp(4, 8))
            .unwrap_or(4);

        // F3: bounded parallelism via chunked scope — avoids 100 threads +
        // 100 ffmpeg processes on large jobs, without needing an async semaphore
        // inside a sync thread::scope.
        let mut processed: Vec<Result<ProcessedSegment, DomainError>> =
            Vec::with_capacity(plans.len());
        for chunk in plans.chunks(max_parallel) {
            let chunk_results: Vec<Result<ProcessedSegment, DomainError>> = std::thread::scope(
                |s| {
                    let mut handles = Vec::with_capacity(chunk.len());
                    for plan in chunk {
                        handles.push(s.spawn(|| {
                    let mut warning = None;
                    let tempo = if plan.target_slot_ms > 0
                        && plan.raw_duration_ms > (plan.target_slot_ms + 80)
                    {
                        let ratio = (plan.raw_duration_ms as f64) / (plan.target_slot_ms as f64);
                        if ratio > 1.50 {
                            let new_duration =
                                (plan.raw_duration_ms as f64 / 1.50).round() as u64;
                            warning = Some(format!(
                                "Segment {} duration ({}ms) exceeded target slot ({}ms) by {:.2}x; clamped time-stretch to 1.50x (new duration: {}ms)",
                                plan.segment_id, plan.raw_duration_ms, plan.target_slot_ms, ratio, new_duration
                            ));
                            Some(1.50)
                        } else {
                            Some(ratio)
                        }
                    } else {
                        None
                    };

                    let dur_ms = Self::process_segment_single_pass(
                        &plan.synth_path,
                        &plan.target_path,
                        tempo,
                    )?;
                    Ok(ProcessedSegment {
                        path: plan.target_path.clone(),
                        duration_ms: dur_ms,
                        warning,
                        start_ms: plan.start_ms,
                        segment_id: plan.segment_id.clone(),
                    })
                }));
                    }
                    handles
                        .into_iter()
                        .map(|h| {
                            h.join().map_err(|_| {
                                DomainError::AlignmentError(
                                    "Audio alignment worker panicked".to_string(),
                                )
                            })?
                        })
                        .collect()
                },
            );
            processed.extend(chunk_results);
        }

        // 3. Assemble timeline sequentially (preserving correct chronology and silence gaps)
        let mut aligned_files = Vec::new();
        let mut quality_warnings = Vec::new();
        let mut current_timeline_ms: u64 = 0;

        for (idx, item_res) in processed.into_iter().enumerate() {
            let item = item_res?;
            if let Some(w) = item.warning {
                quality_warnings.push(w);
            }

            // Check overlap with preceding speech
            if item.start_ms < current_timeline_ms && (current_timeline_ms - item.start_ms) > 200 {
                quality_warnings.push(format!(
                    "Segment {} overlap: target starts at {}ms but previous speech ended at {}ms",
                    item.segment_id, item.start_ms, current_timeline_ms
                ));
            }

            // Pad silence gap between current position and segment start
            if item.start_ms > current_timeline_ms {
                let gap_ms = item.start_ms - current_timeline_ms;
                if gap_ms >= 50 {
                    let silence_path = job_dir.join(format!("silence_{:04}.wav", idx));
                    Self::generate_silence(&silence_path, gap_ms, sample_rate)?;
                    aligned_files.push(silence_path);
                    current_timeline_ms += gap_ms;
                }
            }

            aligned_files.push(item.path);
            current_timeline_ms += item.duration_ms;
        }

        // 4. Post-timeline alignment: if total target duration is provided and current timeline is shorter, pad ending silence
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
