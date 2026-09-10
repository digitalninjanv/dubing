use super::ports::{
    AudioEngine, JobRepository, SpeechSynthesizer, SpeechTranscriber, TextTranslator,
};
use crate::config::{AppSettings, AudioConfig};
use crate::domain::{
    generate_bilingual_txt, generate_srt, generate_vtt, AudioArtifact, AudioFormat, DomainError,
    Job, LanguageRegistry, PipelineStage, SpeakerVoiceConfig, SynthesizedSegment, Transcript,
    TranslatedDocument, TranslationTone, VoiceProfile,
};
use crate::infrastructure::filesystem::{AppPaths, CleanupManager};
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ReviewRequest {
    pub source_transcript: Transcript,
    pub translated: TranslatedDocument,
    pub resume_sender:
        Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<Option<TranslatedDocument>>>>>,
}

impl ReviewRequest {
    pub fn new(
        source_transcript: Transcript,
        translated: TranslatedDocument,
        sender: tokio::sync::oneshot::Sender<Option<TranslatedDocument>>,
    ) -> Self {
        Self {
            source_transcript,
            translated,
            resume_sender: Arc::new(tokio::sync::Mutex::new(Some(sender))),
        }
    }
}

impl std::fmt::Debug for ReviewRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReviewRequest")
            .field("segments_count", &self.translated.segments.len())
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    pub tone: TranslationTone,
    pub voice_config: Option<SpeakerVoiceConfig>,
    pub export_subtitles: bool,
    pub duck_audio: bool,
    pub review_transcript: bool,
    pub review_channel: Option<async_channel::Sender<ReviewRequest>>,
}

/// Atomically persists a small JSON manifest (write .tmp + rename) so a
/// crash can never leave a half-written transcript/translation behind.
fn write_manifest_atomic(path: &std::path::Path, data: &str) -> Result<(), DomainError> {
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, data)
        .map_err(|e| DomainError::Internal(format!("Failed to write manifest tmp file: {}", e)))?;
    std::fs::rename(&tmp_path, path)
        .map_err(|e| DomainError::Internal(format!("Failed to publish manifest file: {}", e)))?;
    Ok(())
}

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

    /// Runs a job from start to finish with default options
    pub async fn run_job<F>(
        &self,
        job: Job,
        cancel_token: CancellationToken,
        on_progress: F,
    ) -> Result<AudioArtifact, DomainError>
    where
        F: Fn(&Job) + Send + Sync + 'static,
    {
        self.run_job_with_options(job, PipelineOptions::default(), cancel_token, on_progress)
            .await
    }

    /// Runs a job from start to finish or resumes from previous stage with custom options
    pub async fn run_job_with_options<F>(
        &self,
        mut job: Job,
        options: PipelineOptions,
        cancel_token: CancellationToken,
        on_progress: F,
    ) -> Result<AudioArtifact, DomainError>
    where
        F: Fn(&Job) + Send + Sync + 'static,
    {
        let total_pipeline_timer = Instant::now();
        let job_dir = AppPaths::job_dir(job.id.as_str());
        std::fs::create_dir_all(&job_dir)
            .map_err(|e| DomainError::Internal(format!("Failed to create job dir: {}", e)))?;

        let auto_cleanup = self.auto_cleanup;
        let debug_mode = self.debug_mode;
        let job_dir_cancel = job_dir.clone();

        // Helper closure to update progress, persist to disk, and notify UI.
        // Resume-tolerant: re-announcing the current stage is a no-op, and a
        // target stage that is already behind us (resume path) only refreshes
        // the message instead of failing the job with an invalid transition.
        let update_stage =
            |job: &mut Job, stage: PipelineStage, msg: &str| -> Result<(), DomainError> {
                if cancel_token.is_cancelled() {
                    job.cancel();
                    if auto_cleanup && !debug_mode {
                        CleanupManager::cleanup_temp_segments(&job_dir_cancel);
                    }
                    return Err(DomainError::Cancelled);
                }
                if job.stage != stage && !job.stage.can_transition_to(&stage) {
                    job.progress.message = msg.to_string();
                    return Ok(());
                }
                job.transition_to(stage).map_err(DomainError::Internal)?;
                job.progress.message = msg.to_string();
                Ok(())
            };

        // 1. Validation Stage
        let t_valid_start = Instant::now();
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
        let t_valid = t_valid_start.elapsed();

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
        let t_transcribe_start = Instant::now();
        let transcript = if job.stage == PipelineStage::Validating
            || job.stage == PipelineStage::Uploading
            || job.stage == PipelineStage::Transcribing
        {
            // If source input is a video container, extract audio track locally first for ultra-fast, lightweight upload
            let audio_for_transcription = if job.source_audio.format.is_video() {
                update_stage(
                    &mut job,
                    PipelineStage::Validating,
                    "Extracting audio track from video container...",
                )?;
                self.job_repo.save(&job).await?;
                on_progress(&job);

                let extracted_path = job_dir.join("extracted_source_audio.mp3");
                if extracted_path.exists() {
                    if let Ok(doc) = self
                        .audio_engine
                        .inspect_and_validate(
                            &extracted_path,
                            self.audio_config.max_file_size_bytes,
                        )
                        .await
                    {
                        doc
                    } else {
                        self.audio_engine
                            .extract_audio(&job.source_audio.path, &extracted_path)
                            .await?
                    }
                } else {
                    self.audio_engine
                        .extract_audio(&job.source_audio.path, &extracted_path)
                        .await?
                }
            } else {
                job.source_audio.clone()
            };

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
                .transcribe(&audio_for_transcription, &job.source_language)
                .await?;

            // If source language was Auto, update job's source language to detected
            if job.source_language.is_auto() {
                job.source_language = t.language.clone();
            }

            // Save transcript manifest (atomic so resume never reads a torn file)
            let transcript_path = job_dir.join("transcript.json");
            let data = serde_json::to_string_pretty(&t)
                .map_err(|e| DomainError::Internal(format!("Serialize error: {}", e)))?;
            write_manifest_atomic(&transcript_path, &data)?;

            t
        } else {
            // Load saved transcript if resuming
            let transcript_path = job_dir.join("transcript.json");
            let content = std::fs::read_to_string(transcript_path)
                .map_err(|e| DomainError::Internal(format!("Failed to load transcript: {}", e)))?;
            serde_json::from_str(&content)
                .map_err(|e| DomainError::Internal(format!("Failed to parse transcript: {}", e)))?
        };
        let t_transcribe = t_transcribe_start.elapsed();

        if let Err(e) = check_cancellation(&mut job) {
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(e);
        }

        // 3. Translation Stage
        let t_translate_start = Instant::now();
        let mut translated = if job.stage == PipelineStage::Transcribing
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
                .translate(&transcript, &job.target_language, options.tone)
                .await?;

            let translated_path = job_dir.join("translated.json");
            let data = serde_json::to_string_pretty(&tr)
                .map_err(|e| DomainError::Internal(format!("Serialize error: {}", e)))?;
            write_manifest_atomic(&translated_path, &data)?;

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
        let t_translate = t_translate_start.elapsed();

        // Interactive Review Checkpoint (Human-in-the-Loop)
        if options.review_transcript {
            if let Some(ref ch) = options.review_channel {
                let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
                let req = ReviewRequest::new(transcript.clone(), translated.clone(), resume_tx);
                if ch.send(req).await.is_ok() {
                    match resume_rx.await {
                        Ok(Some(edited_doc)) => {
                            tracing::info!(
                                "User submitted reviewed translation with {} segments",
                                edited_doc.segments.len()
                            );
                            translated = edited_doc;
                            // Save updated translation to disk (atomic)
                            let translated_path = job_dir.join("translated.json");
                            if let Ok(data) = serde_json::to_string_pretty(&translated) {
                                if let Err(e) = write_manifest_atomic(&translated_path, &data) {
                                    tracing::warn!("Failed to persist reviewed translation: {}", e);
                                }
                            }
                        }
                        Ok(None) => {
                            tracing::info!(
                                "User cancelled job during translation review checkpoint"
                            );
                            job.cancel();
                            self.job_repo.save(&job).await?;
                            on_progress(&job);
                            return Err(DomainError::Cancelled);
                        }
                        Err(_) => {
                            tracing::warn!(
                                "Review channel dropped, proceeding with existing translation"
                            );
                        }
                    }
                }
            }
        }

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

        let default_voice_pool = ["Kore", "Puck", "Aoede", "Charon", "Fenrir"];
        let mut ordered_voices = vec![base_default_voice.to_string()];
        for v in &default_voice_pool {
            if *v != base_default_voice {
                ordered_voices.push(v.to_string());
            }
        }

        let mut voice_map: HashMap<String, VoiceProfile> = HashMap::new();
        let unique_speakers = transcript.unique_speakers();
        for (idx, spk) in unique_speakers.iter().enumerate() {
            let voice_name = options
                .voice_config
                .as_ref()
                .and_then(|c| {
                    if idx == 0 {
                        c.speaker_1_voice
                            .as_deref()
                            .or_else(|| c.get_voice_for(Some(spk.as_str())))
                    } else if idx == 1 {
                        c.speaker_2_voice
                            .as_deref()
                            .or_else(|| c.get_voice_for(Some(spk.as_str())))
                    } else {
                        c.get_voice_for(Some(spk.as_str()))
                    }
                })
                .map(|s| s.to_string())
                .unwrap_or_else(|| ordered_voices[idx % ordered_voices.len()].clone());

            voice_map.insert(
                spk.clone(),
                VoiceProfile {
                    id: format!("voice_{}", idx),
                    voice_name,
                    language: job.target_language.as_str().to_string(),
                    style: None,
                    speed: 1.0,
                },
            );
        }

        let default_voice_name = options
            .voice_config
            .as_ref()
            .and_then(|c| c.get_voice_for(None))
            .map(|s| s.to_string())
            .unwrap_or_else(|| base_default_voice.to_string());

        let default_voice = VoiceProfile {
            id: "default".to_string(),
            voice_name: default_voice_name,
            language: job.target_language.as_str().to_string(),
            style: None,
            speed: 1.0,
        };

        let t_synthesis_start = Instant::now();
        let mut tasks = Vec::with_capacity(translated.segments.len());

        for (idx, segment) in translated.segments.iter().enumerate() {
            let seg = segment.clone();
            let voice = segment
                .speaker_id
                .as_ref()
                .and_then(|s| voice_map.get(s))
                .unwrap_or(&default_voice)
                .clone();
            let segment_output_file = job_dir.join(format!("seg_{:04}.wav", idx + 1));
            let synth = self.synthesizer.clone();
            let engine = self.audio_engine.clone();
            let cancel = cancel_token.clone();

            tasks.push(async move {
                if cancel.is_cancelled() {
                    return Err(DomainError::Cancelled);
                }

                // Resume capability: reuse existing file without ffprobe if
                // size >0 (R3). Probe only as fallback to avoid 60 probes on resume.
                if segment_output_file.exists() {
                    if let Ok(meta) = std::fs::metadata(&segment_output_file) {
                        if meta.len() > 1024 {
                            // Try to infer duration from file size quickly; if
                            // probing was done before, seg files are valid.
                            // Keep a lightweight metadata check, probe only if needed.
                            if let Ok(probe_meta) = engine.probe(&segment_output_file).await {
                                if probe_meta.duration_ms > 0 {
                                    return Ok((
                                        idx,
                                        SynthesizedSegment {
                                            segment_id: seg.segment_id,
                                            speaker_id: seg.speaker_id,
                                            path: segment_output_file,
                                            duration_ms: probe_meta.duration_ms,
                                        },
                                    ));
                                }
                            } else if meta.len() > 4096 {
                                // Fallback: assume ~1s per 32kB for 24kHz mono as estimate
                                // to avoid probe cost on bulk resume (probe only on final validation).
                                let est_ms = meta.len() / 32;
                                if est_ms > 200 {
                                    return Ok((
                                        idx,
                                        SynthesizedSegment {
                                            segment_id: seg.segment_id,
                                            speaker_id: seg.speaker_id,
                                            path: segment_output_file.clone(),
                                            duration_ms: est_ms,
                                        },
                                    ));
                                }
                            }
                        }
                    }
                }

                if cancel.is_cancelled() {
                    return Err(DomainError::Cancelled);
                }

                // Request pacing: token-bucket style — uniform small delay per
                // segment avoids burst 429s without the odd 0/350/0/350 pattern
                // that added ~10s dead time on 60 segments (F2).
                if idx > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                }

                let synth_result = synth
                    .synthesize_segment(&seg, &voice, &segment_output_file)
                    .await?;

                Ok((idx, synth_result))
            });
        }

        // Controlled concurrency = 2 as per PRD Section 23 to remain well within Free Tier RPM and prevent 429 burst errors
        const TTS_CONCURRENCY: usize = 2;
        let mut stream = stream::iter(tasks).buffer_unordered(TTS_CONCURRENCY);
        let mut collected: Vec<(usize, SynthesizedSegment)> =
            Vec::with_capacity(translated.segments.len());

        while let Some(res) = stream.next().await {
            let (idx, synth_result) = match res {
                Ok(item) => item,
                Err(e) => {
                    if cancel_token.is_cancelled() {
                        job.cancel();
                        if auto_cleanup && !debug_mode {
                            CleanupManager::cleanup_temp_segments(&job_dir_cancel);
                        }
                        self.job_repo.save(&job).await?;
                        on_progress(&job);
                        return Err(DomainError::Cancelled);
                    } else {
                        job.fail(true, e.to_string());
                        self.job_repo.save(&job).await?;
                        on_progress(&job);
                        return Err(e);
                    }
                }
            };

            collected.push((idx, synth_result));
            job.progress.completed_segments += 1;
            job.progress.message = format!(
                "Synthesized segment {} of {}",
                job.progress.completed_segments, job.progress.total_segments
            );
            // Persist progress periodically (every 5 segments + final) to
            // avoid hundreds of job.json writes on large jobs (F2). Resume
            // still works because seg_*.wav files are the source of truth.
            if job.progress.completed_segments.is_multiple_of(5)
                || job.progress.completed_segments == job.progress.total_segments
            {
                self.job_repo.save(&job).await?;
            }
            on_progress(&job);
        }

        collected.sort_by_key(|(idx, _)| *idx);
        let synthesized_segments: Vec<SynthesizedSegment> =
            collected.into_iter().map(|(_, seg)| seg).collect();
        let t_synthesis = t_synthesis_start.elapsed();

        if let Err(e) = check_cancellation(&mut job) {
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(e);
        }

        // 5. Alignment Stage
        let t_align_start = Instant::now();
        update_stage(
            &mut job,
            PipelineStage::Aligning,
            "Aligning audio timeline and silence...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        let source_duration_ms = job.source_audio.metadata.duration_ms;

        let alignment_res = self
            .audio_engine
            .align_segments(
                &job_dir,
                &transcript.segments,
                &synthesized_segments,
                Some(source_duration_ms),
            )
            .await?;
        let t_align = t_align_start.elapsed();

        // 6. Exporting Stage
        let t_export_start = Instant::now();
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

        let mut artifact = self
            .audio_engine
            .export_final(
                &alignment_res.aligned_files,
                &final_output_path,
                AudioFormat::Mp3,
                self.audio_config.default_bitrate_kbps,
                alignment_res.quality_warnings,
                Some(source_duration_ms),
            )
            .await?;
        let t_export = t_export_start.elapsed();

        // Generate Subtitle artifacts (.srt, .vtt, bilingual .txt)
        let file_stem = job
            .source_audio
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio");

        let srt_path = AppPaths::outputs_dir().join(format!(
            "{}_{}.srt",
            file_stem,
            job.target_language.as_str()
        ));
        let vtt_path = AppPaths::outputs_dir().join(format!(
            "{}_{}.vtt",
            file_stem,
            job.target_language.as_str()
        ));
        let txt_path = AppPaths::outputs_dir().join(format!(
            "{}_{}_bilingual.txt",
            file_stem,
            job.target_language.as_str()
        ));

        if let Ok(()) = std::fs::write(&srt_path, generate_srt(&translated)) {
            artifact.subtitle_srt_path = Some(srt_path);
        }
        if let Ok(()) = std::fs::write(&vtt_path, generate_vtt(&translated)) {
            artifact.subtitle_vtt_path = Some(vtt_path);
        }
        if let Ok(()) = std::fs::write(&txt_path, generate_bilingual_txt(&translated)) {
            artifact.transcript_txt_path = Some(txt_path);
        }

        // Audio Ducking (Smooth BGM attenuation)
        let mut audio_for_packaging = artifact.path.clone();
        if options.duck_audio {
            let ducked_output = job_dir.join(format!("ducked_mix.{}", artifact.format.extension()));
            match self
                .audio_engine
                .mix_with_ducking(&job.source_audio.path, &artifact.path, &ducked_output)
                .await
            {
                Ok(dp) => {
                    tracing::info!("Audio ducking generated: {}", dp.display());
                    audio_for_packaging = dp.clone();
                    if !job.source_audio.format.is_video() {
                        artifact.path = dp;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to duck audio, keeping dubbed track: {}", e);
                    artifact
                        .quality_warnings
                        .push(format!("Audio ducking failed: {}", e));
                }
            }
        }

        // Fast video remuxing with soft subtitles if source input is video container
        if job.source_audio.format.is_video() {
            let ext = job.source_audio.format.extension();
            let remuxed_video_path = AppPaths::outputs_dir().join(format!(
                "{}_{}_dubbed.{}",
                file_stem,
                job.target_language.as_str(),
                ext
            ));
            match self
                .audio_engine
                .remux_video(
                    &job.source_audio.path,
                    &audio_for_packaging,
                    artifact.subtitle_srt_path.as_deref(),
                    Some(job.target_language.to_iso639_2()),
                    &remuxed_video_path,
                )
                .await
            {
                Ok(vp) => {
                    tracing::info!(
                        "Remuxed dubbed video generated with soft subtitles at: {}",
                        vp.display()
                    );
                    artifact.video_path = Some(vp);
                }
                Err(e) => {
                    tracing::warn!("Failed to remux dubbed video: {}", e);
                    artifact
                        .quality_warnings
                        .push(format!("Video remuxing failed: {}", e));
                }
            }
        }

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

        tracing::info!(
            "Pipeline finished for Job {} in {:.2}s [Validation: {:.2}s, Transcription: {:.2}s, Translation: {:.2}s, Synthesis: {:.2}s ({} segs), Alignment: {:.2}s, Export: {:.2}s]",
            job.id.as_str(),
            total_pipeline_timer.elapsed().as_secs_f64(),
            t_valid.as_secs_f64(),
            t_transcribe.as_secs_f64(),
            t_translate.as_secs_f64(),
            t_synthesis.as_secs_f64(),
            translated.segments.len(),
            t_align.as_secs_f64(),
            t_export.as_secs_f64()
        );

        Ok(artifact)
    }
}
