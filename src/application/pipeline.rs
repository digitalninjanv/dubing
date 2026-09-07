use super::ports::{
    AudioEngine, JobRepository, LiveSpeechTranslator, SpeechSynthesizer, SpeechTranscriber,
    TextTranslator,
};
use crate::config::{AppSettings, AudioConfig};
use crate::domain::{
    generate_bilingual_txt, generate_srt, generate_vtt, AudioArtifact, AudioFormat, DomainError,
    DubbingEngine, Job, LanguageRegistry, PipelineStage, SpeakerVoiceConfig, SynthesizedSegment,
    TranslationTone, VoiceProfile,
};
use crate::infrastructure::filesystem::{AppPaths, CleanupManager};
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    pub tone: TranslationTone,
    pub voice_config: Option<SpeakerVoiceConfig>,
    pub export_subtitles: bool,
    pub engine: DubbingEngine,
}

pub struct PipelineOrchestrator {
    transcriber: Arc<dyn SpeechTranscriber>,
    translator: Arc<dyn TextTranslator>,
    synthesizer: Arc<dyn SpeechSynthesizer>,
    live_translator: Option<Arc<dyn LiveSpeechTranslator>>,
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
            live_translator: None,
            audio_engine,
            job_repo,
            audio_config,
            auto_cleanup: true,
            debug_mode: false,
        }
    }

    pub fn with_live_translator(mut self, live_translator: Arc<dyn LiveSpeechTranslator>) -> Self {
        self.live_translator = Some(live_translator);
        self
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
            live_translator: None,
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
        if options.engine == DubbingEngine::LiveTranslate {
            if let Some(ref live_trans) = self.live_translator {
                return self
                    .run_live_translate_job(job, options, live_trans, cancel_token, on_progress)
                    .await;
            } else {
                tracing::warn!(
                    "Live Translate engine requested but not configured on orchestrator, falling back to Studio pipeline"
                );
            }
        }

        let total_pipeline_timer = Instant::now();
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
        let t_transcribe = t_transcribe_start.elapsed();

        if let Err(e) = check_cancellation(&mut job) {
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(e);
        }

        // 3. Translation Stage
        let t_translate_start = Instant::now();
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
                .translate(&transcript, &job.target_language, options.tone)
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
        let t_translate = t_translate_start.elapsed();

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
            let voice_name = options
                .voice_config
                .as_ref()
                .and_then(|c| c.get_voice_for(Some(spk.as_str())))
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

                // Resume capability: if segment already exists and is valid, reuse it!
                if segment_output_file.exists() {
                    if let Ok(meta) = engine.probe(&segment_output_file).await {
                        if meta.duration_ms > 0 {
                            return Ok((
                                idx,
                                SynthesizedSegment {
                                    segment_id: seg.segment_id,
                                    speaker_id: seg.speaker_id,
                                    path: segment_output_file,
                                    duration_ms: meta.duration_ms,
                                },
                            ));
                        }
                    }
                }

                if cancel.is_cancelled() {
                    return Err(DomainError::Cancelled);
                }

                // Request pacing: gentle stagger between segment dispatches to avoid burst rate spikes
                if idx > 0 {
                    let stagger_ms = ((idx % 2) as u64) * 350;
                    if stagger_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(stagger_ms)).await;
                    }
                }

                let synth_result = synth
                    .synthesize_segment(&seg, &voice, &segment_output_file)
                    .await?;

                Ok((idx, synth_result))
            });
        }

        // Controlled concurrency = 2 as per PRD Section 23 to remain well within Free Tier RPM and prevent 429 errors
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
            self.job_repo.save(&job).await?;
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

        // Fast video remuxing if source input is video container
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
                .remux_video(&job.source_audio.path, &artifact.path, &remuxed_video_path)
                .await
            {
                Ok(vp) => {
                    tracing::info!("Remuxed dubbed video generated at: {}", vp.display());
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

    /// Executes speech-to-speech translation using the real-time Gemini Live API (gemini-3.5-live-translate-preview).
    /// Streams audio via WebSocket and re-encodes the 24kHz raw PCM output into a final MP3/WAV file.
    async fn run_live_translate_job<F>(
        &self,
        mut job: Job,
        _options: PipelineOptions,
        live_translator: &Arc<dyn LiveSpeechTranslator>,
        cancel_token: CancellationToken,
        on_progress: F,
    ) -> Result<AudioArtifact, DomainError>
    where
        F: Fn(&Job) + Send + Sync + 'static,
    {
        let total_timer = Instant::now();
        let job_dir = AppPaths::job_dir(job.id.as_str());
        std::fs::create_dir_all(&job_dir)
            .map_err(|e| DomainError::Internal(format!("Failed to create job dir: {}", e)))?;

        let auto_cleanup = self.auto_cleanup;
        let debug_mode = self.debug_mode;
        let job_dir_cancel = job_dir.clone();

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
        update_stage(
            &mut job,
            PipelineStage::Validating,
            "Validating audio input for Live Translate...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        // 2. Uploading / Session Setup Stage
        update_stage(
            &mut job,
            PipelineStage::Uploading,
            "Connecting to Gemini 3.5 Live Translate WebSocket...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        // 3. Transcribing / Streaming Stage
        update_stage(
            &mut job,
            PipelineStage::Transcribing,
            "Streaming 16kHz audio chunks to Gemini Live API...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        // 4. Translating & Synthesizing in Real-Time
        update_stage(
            &mut job,
            PipelineStage::Translating,
            "Translating and synthesizing speech in real-time...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        let live_res = match live_translator
            .translate_speech(
                &job.source_audio.path,
                &job.target_language,
                &job_dir,
                &cancel_token,
            )
            .await
        {
            Ok(res) => res,
            Err(e) => {
                let is_retryable = e.is_retryable();
                job.fail(is_retryable, e.to_string());
                let _ = self.job_repo.save(&job).await;
                on_progress(&job);
                return Err(e);
            }
        };

        // 5. Synthesis Complete
        update_stage(
            &mut job,
            PipelineStage::Synthesizing,
            "Receiving synthesized 24kHz audio stream...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        // 6. Aligning Stage
        update_stage(
            &mut job,
            PipelineStage::Aligning,
            "Finalizing audio stream alignment...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        // 7. Exporting Stage: encode raw 24kHz PCM to MP3 / WAV
        update_stage(
            &mut job,
            PipelineStage::Exporting,
            "Encoding high-fidelity output MP3 with FFmpeg...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        let output_extension = if self.audio_config.export_wav { "wav" } else { "mp3" };
        let output_audio_path = job_dir.join(format!("dubbed_output.{}", output_extension));
        let bitrate_str = format!("{}k", self.audio_config.default_bitrate_kbps);

        let mut ffmpeg_cmd = std::process::Command::new("ffmpeg");
        ffmpeg_cmd.args([
            "-y",
            "-f",
            "s16le",
            "-ar",
            "24000",
            "-ac",
            "1",
            "-i",
            live_res.raw_pcm_path.to_str().unwrap_or_default(),
        ]);

        if self.audio_config.export_wav {
            ffmpeg_cmd.args(["-c:a", "pcm_s16le"]);
        } else {
            ffmpeg_cmd.args(["-c:a", "libmp3lame", "-b:a", &bitrate_str]);
        }

        ffmpeg_cmd.arg(output_audio_path.to_str().unwrap_or_default());

        let export_status = ffmpeg_cmd.output().map_err(|e| {
            DomainError::Internal(format!("Failed to execute FFmpeg for export: {}", e))
        })?;

        if !export_status.status.success() {
            let err = DomainError::Internal(format!(
                "FFmpeg audio encoding failed: {}",
                String::from_utf8_lossy(&export_status.stderr)
            ));
            job.fail(false, err.to_string());
            let _ = self.job_repo.save(&job).await;
            on_progress(&job);
            return Err(err);
        }

        // Multiplex with video if original file is video
        let mut dubbed_video_path = None;
        if job.source_audio.format.is_video() {
            let video_output = job_dir.join("dubbed_video.mp4");
            let mux_status = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-i",
                    job.source_audio.path.to_str().unwrap_or_default(),
                    "-i",
                    output_audio_path.to_str().unwrap_or_default(),
                    "-map",
                    "0:v:0",
                    "-map",
                    "1:a:0",
                    "-c:v",
                    "copy",
                    "-c:a",
                    "aac",
                    "-shortest",
                    video_output.to_str().unwrap_or_default(),
                ])
                .output();

            if let Ok(res) = mux_status {
                if res.status.success() && video_output.exists() {
                    dubbed_video_path = Some(video_output);
                }
            }
        }

        // Save bilingual script if transcripts were emitted
        let mut transcript_txt_path = None;
        if !live_res.output_transcripts.is_empty() || !live_res.input_transcripts.is_empty() {
            let txt_path = job_dir.join("bilingual_script.txt");
            let mut script_content = String::new();
            script_content.push_str(&format!(
                "# AudioDub AI — Gemini 3.5 Live Speech Translation\n# Source: {} | Target: {}\n\n",
                job.source_language.as_str(),
                job.target_language.as_str()
            ));

            let max_lines = live_res.input_transcripts.len().max(live_res.output_transcripts.len());
            for i in 0..max_lines {
                let orig = live_res.input_transcripts.get(i).map(|s| s.as_str()).unwrap_or("");
                let trans = live_res.output_transcripts.get(i).map(|s| s.as_str()).unwrap_or("");
                if !orig.is_empty() {
                    script_content.push_str(&format!("[{}] {}\n", job.source_language.as_str(), orig));
                }
                if !trans.is_empty() {
                    script_content.push_str(&format!("[{}] {}\n\n", job.target_language.as_str(), trans));
                }
            }

            if std::fs::write(&txt_path, script_content).is_ok() {
                transcript_txt_path = Some(txt_path);
            }
        }

        // 8. Validating Output Stage
        update_stage(
            &mut job,
            PipelineStage::ValidatingOutput,
            "Validating final audio output...",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        let inspected_output = self
            .audio_engine
            .inspect_and_validate(&output_audio_path, self.audio_config.max_file_size_bytes)
            .await?;

        let artifact = AudioArtifact {
            path: output_audio_path,
            format: if self.audio_config.export_wav {
                AudioFormat::Wav
            } else {
                AudioFormat::Mp3
            },
            duration_ms: inspected_output.metadata.duration_ms,
            size_bytes: inspected_output.size_bytes,
            quality_warnings: vec![],
            subtitle_srt_path: None,
            subtitle_vtt_path: None,
            transcript_txt_path,
            video_path: dubbed_video_path,
        };

        // 9. Completed Stage
        update_stage(
            &mut job,
            PipelineStage::Completed,
            "Live Speech Translation completed successfully!",
        )?;
        self.job_repo.save(&job).await?;
        on_progress(&job);

        if auto_cleanup && !debug_mode {
            CleanupManager::cleanup_temp_segments(&job_dir);
        }

        tracing::info!(
            "Gemini Live Translate finished for Job {} in {:.2}s",
            job.id.as_str(),
            total_timer.elapsed().as_secs_f64()
        );

        Ok(artifact)
    }
}
