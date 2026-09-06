use super::ports::{
    AudioEngine, JobRepository, SpeechSynthesizer, SpeechTranscriber, TextTranslator,
};
use crate::config::{AppSettings, AudioConfig};
use crate::domain::{
    AudioArtifact, AudioFormat, DomainError, Job, LanguageRegistry, PipelineStage,
    SynthesizedSegment, VoiceProfile,
};
use crate::infrastructure::filesystem::{AppPaths, CleanupManager};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub struct PipelineOrchestrator {
    transcriber: Arc<dyn SpeechTranscriber>,
    translator: Arc<dyn TextTranslator>,
    synthesizer: Arc<dyn SpeechSynthesizer>,
    audio_engine: Arc<dyn AudioEngine>,
    job_repo: Arc<dyn JobRepository>,
    audio_config: AudioConfig,
    auto_cleanup: bool,
    debug_mode: bool,
}

impl PipelineOrchestrator {
    pub fn new(
        transcriber: Arc<dyn SpeechTranscriber>,
        translator: Arc<dyn TextTranslator>,
        synthesizer: Arc<dyn SpeechSynthesizer>,
        audio_engine: Arc<dyn AudioEngine>,
        job_repo: Arc<dyn JobRepository>,
        audio_config: AudioConfig,
    ) -> Self {
        Self {
            transcriber,
            translator,
            synthesizer,
            audio_engine,
            job_repo,
            audio_config,
            auto_cleanup: true,
            debug_mode: false,
        }
    }

    pub fn with_settings(
        transcriber: Arc<dyn SpeechTranscriber>,
        translator: Arc<dyn TextTranslator>,
        synthesizer: Arc<dyn SpeechSynthesizer>,
        audio_engine: Arc<dyn AudioEngine>,
        job_repo: Arc<dyn JobRepository>,
        settings: &AppSettings,
    ) -> Self {
        Self {
            transcriber,
            translator,
            synthesizer,
            audio_engine,
            job_repo,
            audio_config: settings.audio.clone(),
            auto_cleanup: settings.auto_cleanup,
            debug_mode: settings.debug_mode,
        }
    }

    /// Runs a job from start to finish or resumes from previous stage
    pub async fn run_job<F>(
        &self,
        mut job: Job,
        cancel_token: CancellationToken,
        on_progress: F,
    ) -> Result<AudioArtifact, DomainError>
    where
        F: Fn(&Job) + Send + Sync + 'static,
    {
        let job_dir = AppPaths::job_dir(job.id.as_str());
        std::fs::create_dir_all(&job_dir)
            .map_err(|e| DomainError::Internal(format!("Failed to create job dir: {}", e)))?;

        let auto_cleanup = self.auto_cleanup;
        let debug_mode = self.debug_mode;
        let job_dir_cancel = job_dir.clone();

        // Helper closure to update progress, persist to disk, and notify UI
        let update_stage =
            |job: &mut Job, stage: PipelineStage, msg: &str| -> Result<(), DomainError> {
                if cancel_token.is_cancelled() {
                    job.cancel();
                    if auto_cleanup && !debug_mode {
                        CleanupManager::cleanup_temp_segments(&job_dir_cancel);
                    }
                    return Err(DomainError::Cancelled);
                }
                job.transition_to(stage).map_err(DomainError::Internal)?;
                job.progress.message = msg.to_string();
                Ok(())
            };

        // 1. Validation Stage
        if job.stage == PipelineStage::Idle || job.stage == PipelineStage::Validating {
            update_stage(
                &mut job,
                PipelineStage::Validating,
                "Validating input audio...",
            )?;
            self.job_repo.save(&job).await?;
            on_progress(&job);

            job.source_audio
                .validate_for_processing(self.audio_config.max_file_size_bytes)
                .map_err(DomainError::InvalidAudio)?;
        }

        let check_cancellation = |job: &mut Job| -> Result<(), DomainError> {
            if cancel_token.is_cancelled() {
                job.cancel();
                if auto_cleanup && !debug_mode {
                    CleanupManager::cleanup_temp_segments(&job_dir_cancel);
                }
                Err(DomainError::Cancelled)
            } else {
                Ok(())
            }
        };

        // Check cancellation
        if let Err(e) = check_cancellation(&mut job) {
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(e);
        }

        // 2. Uploading & Transcription Stage
        let transcript = if job.stage == PipelineStage::Validating
            || job.stage == PipelineStage::Uploading
            || job.stage == PipelineStage::Transcribing
        {
            update_stage(
                &mut job,
                PipelineStage::Uploading,
                "Uploading audio to Gemini...",
            )?;
            self.job_repo.save(&job).await?;
            on_progress(&job);

            update_stage(
                &mut job,
                PipelineStage::Transcribing,
                "Transcribing speech with diarization...",
            )?;
            self.job_repo.save(&job).await?;
            on_progress(&job);

            let t = self
                .transcriber
                .transcribe(&job.source_audio, &job.source_language)
                .await?;

            // If source language was Auto, update job's source language to detected
            if job.source_language.is_auto() {
                job.source_language = t.language.clone();
            }

            // Save transcript manifest
            let transcript_path = job_dir.join("transcript.json");
            let data = serde_json::to_string_pretty(&t)
                .map_err(|e| DomainError::Internal(format!("Serialize error: {}", e)))?;
            let _ = std::fs::write(transcript_path, data);

            t
        } else {
            // Load saved transcript if resuming
            let transcript_path = job_dir.join("transcript.json");
            let content = std::fs::read_to_string(transcript_path)
                .map_err(|e| DomainError::Internal(format!("Failed to load transcript: {}", e)))?;
            serde_json::from_str(&content)
                .map_err(|e| DomainError::Internal(format!("Failed to parse transcript: {}", e)))?
        };

        if let Err(e) = check_cancellation(&mut job) {
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(e);
        }

        // 3. Translation Stage
        let translated = if job.stage == PipelineStage::Transcribing
            || job.stage == PipelineStage::Translating
        {
            update_stage(
                &mut job,
                PipelineStage::Translating,
                "Translating transcript to target language...",
            )?;
            self.job_repo.save(&job).await?;
            on_progress(&job);

            let tr = self
                .translator
                .translate(&transcript, &job.target_language)
                .await?;

            let translated_path = job_dir.join("translated.json");
            let data = serde_json::to_string_pretty(&tr)
                .map_err(|e| DomainError::Internal(format!("Serialize error: {}", e)))?;
            let _ = std::fs::write(translated_path, data);

            tr
        } else {
            let translated_path = job_dir.join("translated.json");
            let content = std::fs::read_to_string(translated_path).map_err(|e| {
                DomainError::Internal(format!("Failed to load translated doc: {}", e))
            })?;
            serde_json::from_str(&content).map_err(|e| {
                DomainError::Internal(format!("Failed to parse translated doc: {}", e))
            })?
        };

        if let Err(e) = check_cancellation(&mut job) {
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(e);
        }

        // 4. Speech Synthesis Stage (Utterance Chunking with Speaker Awareness)
        update_stage(
            &mut job,
            PipelineStage::Synthesizing,
            "Generating voice audio per segment...",
        )?;
        job.progress.total_segments = translated.segments.len();
        job.progress.completed_segments = 0;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        // Voice profiles mapping for single and multi-speaker
        let registry = LanguageRegistry::standard();
        let target_lang_info = registry.get(&job.target_language);
        let base_default_voice = target_lang_info
            .map(|info| info.default_voice.as_str())
            .unwrap_or("Kore");

        let default_voice_pool = ["Kore", "Puck", "Fenrir", "Aoede"];
        let mut ordered_voices = vec![base_default_voice.to_string()];
        for v in &default_voice_pool {
            if *v != base_default_voice {
                ordered_voices.push(v.to_string());
            }
        }

        let mut voice_map: HashMap<String, VoiceProfile> = HashMap::new();
        let unique_speakers = transcript.unique_speakers();
        for (idx, spk) in unique_speakers.iter().enumerate() {
            let voice_name = &ordered_voices[idx % ordered_voices.len()];
            voice_map.insert(
                spk.clone(),
                VoiceProfile {
                    id: format!("voice_{}", idx),
                    voice_name: voice_name.clone(),
                    language: job.target_language.as_str().to_string(),
                    style: None,
                    speed: 1.0,
                },
            );
        }

        let default_voice = VoiceProfile {
            id: "default".to_string(),
            voice_name: base_default_voice.to_string(),
            language: job.target_language.as_str().to_string(),
            style: None,
            speed: 1.0,
        };

        let mut synthesized_segments: Vec<SynthesizedSegment> = Vec::new();
        let semaphore = Arc::new(Semaphore::new(2)); // Controlled concurrency = 2 as per PRD Section 23

        for (idx, segment) in translated.segments.iter().enumerate() {
            if let Err(e) = check_cancellation(&mut job) {
                self.job_repo.save(&job).await?;
                on_progress(&job);
                return Err(e);
            }

            let segment_output_file = job_dir.join(format!("seg_{:04}.wav", idx + 1));

            // Resume capability: if segment already exists and is valid, reuse it!
            if segment_output_file.exists() {
                if let Ok(meta) = self.audio_engine.probe(&segment_output_file).await {
                    if meta.duration_ms > 0 {
                        synthesized_segments.push(SynthesizedSegment {
                            segment_id: segment.segment_id.clone(),
                            speaker_id: segment.speaker_id.clone(),
                            path: segment_output_file,
                            duration_ms: meta.duration_ms,
                        });
                        job.progress.completed_segments += 1;
                        on_progress(&job);
                        continue;
                    }
                }
            }

            let voice = segment
                .speaker_id
                .as_ref()
                .and_then(|s| voice_map.get(s))
                .unwrap_or(&default_voice)
                .clone();

            let _permit = semaphore
                .acquire()
                .await
                .map_err(|e| DomainError::Internal(format!("Semaphore error: {}", e)))?;

            let synth_result = self
                .synthesizer
                .synthesize_segment(segment, &voice, &segment_output_file)
                .await?;

            synthesized_segments.push(synth_result);
            job.progress.completed_segments += 1;
            job.progress.message = format!(
                "Synthesized segment {} of {}",
                job.progress.completed_segments, job.progress.total_segments
            );
            self.job_repo.save(&job).await?;
            on_progress(&job);
        }

        if let Err(e) = check_cancellation(&mut job) {
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(e);
        }

        // 5. Alignment Stage
        update_stage(
            &mut job,
            PipelineStage::Aligning,
            "Aligning audio timeline and silence...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        let alignment_res = self
            .audio_engine
            .align_segments(&job_dir, &transcript.segments, &synthesized_segments)
            .await?;

        // 6. Exporting Stage
        update_stage(
            &mut job,
            PipelineStage::Exporting,
            "Encoding final MP3 output...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        let output_file_name = format!(
            "{}_{}.mp3",
            job.source_audio
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("audio"),
            job.target_language.as_str()
        );
        let final_output_path = AppPaths::outputs_dir().join(&output_file_name);

        let artifact = self
            .audio_engine
            .export_final(
                &alignment_res.aligned_files,
                &final_output_path,
                AudioFormat::Mp3,
                self.audio_config.default_bitrate_kbps,
                alignment_res.quality_warnings,
            )
            .await?;

        // 7. Validating Output (Quality Gate)
        update_stage(
            &mut job,
            PipelineStage::ValidatingOutput,
            "Validating output file integrity...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        if artifact.duration_ms == 0 || artifact.size_bytes == 0 {
            let err = DomainError::ExportError(
                "Final output validation failed: 0 duration or size".to_string(),
            );
            job.fail(false, err.to_string());
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(err);
        }

        // 8. Completed Stage
        update_stage(
            &mut job,
            PipelineStage::Completed,
            "Translation completed successfully!",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        // Auto cleanup of temporary segment files if configured
        if self.auto_cleanup && !self.debug_mode {
            CleanupManager::cleanup_temp_segments(&job_dir);
        }

        Ok(artifact)
    }
}
