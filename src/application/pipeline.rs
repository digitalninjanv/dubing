use super::ports::{
    AudioEngine, JobRepository, SpeechSynthesizer, SpeechTranscriber, TextTranslator,
};
use crate::application::quality_gate::QualityGate;
use crate::config::{AppSettings, AudioConfig};
use crate::domain::{
    generate_bilingual_txt, generate_srt, generate_vtt, AudioArtifact, AudioFormat, DomainError,
    Job, LanguageRegistry, PipelineStage, SpeakerVoiceConfig, SynthesizedSegment, Transcript,
    TranslatedDocument, TranslationTone, VoiceProfile,
};
use crate::infrastructure::filesystem::{
    fingerprint, write_atomic, AppPaths, ArtifactManifest, ArtifactStore, CleanupManager,
    PROVENANCE_SCHEMA_VERSION,
};
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

pub struct PipelineOrchestrator {
    transcriber: Arc<dyn SpeechTranscriber>,
    translator: Arc<dyn TextTranslator>,
    synthesizer: Arc<dyn SpeechSynthesizer>,
    audio_engine: Arc<dyn AudioEngine>,
    job_repo: Arc<dyn JobRepository>,
    audio_config: AudioConfig,
    auto_cleanup: bool,
    debug_mode: bool,
    tts_concurrency: usize,
    tts_request_spacing_ms: u64,
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
            tts_concurrency: 2,
            tts_request_spacing_ms: 120,
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
        let runtime = settings.runtime.clone().normalized();
        Self {
            transcriber,
            translator,
            synthesizer,
            audio_engine,
            job_repo,
            audio_config: settings.audio.clone(),
            auto_cleanup: settings.auto_cleanup,
            debug_mode: settings.debug_mode,
            tts_concurrency: runtime.tts_concurrency,
            tts_request_spacing_ms: runtime.tts_request_spacing_ms,
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
        let artifact_store = ArtifactStore::new(&job_dir);
        let mut artifact_manifest = match artifact_store.load() {
            Ok(manifest) => manifest,
            Err(err) => {
                tracing::warn!(
                    "Ignoring invalid artifact manifest for job {} and rebuilding it: {}",
                    job.id,
                    err
                );
                ArtifactManifest::default()
            }
        };

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
            write_atomic(&transcript_path, data.as_bytes())?;

            t
        } else {
            // Load saved transcript if resuming
            let transcript_path = job_dir.join("transcript.json");
            let content = std::fs::read_to_string(transcript_path)
                .map_err(|e| DomainError::Internal(format!("Failed to load transcript: {}", e)))?;
            serde_json::from_str(&content)
                .map_err(|e| DomainError::Internal(format!("Failed to parse transcript: {}", e)))?
        };
        QualityGate::validate_transcript(&transcript)?;
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

            let translation_provenance = fingerprint(&serde_json::json!({
                "schema": PROVENANCE_SCHEMA_VERSION,
                "stage": "translation",
                "transcript": &transcript,
                "target_language": &job.target_language,
                "tone": &options.tone,
                "provider": self.translator.cache_identity(),
            }))
            .map_err(|e| {
                DomainError::Internal(format!(
                    "Failed to fingerprint translation provenance: {}",
                    e
                ))
            })?;

            let translated_path = job_dir.join("translated.json");
            let data = serde_json::to_string_pretty(&tr)
                .map_err(|e| DomainError::Internal(format!("Serialize error: {}", e)))?;
            write_atomic(&translated_path, data.as_bytes())?;
            artifact_store.register_with_provenance(
                "translation",
                &translated_path,
                &mut artifact_manifest,
                Some(translation_provenance.clone()),
            )?;
            artifact_store.save(&artifact_manifest)?;

            tr
        } else {
            let translation_provenance = fingerprint(&serde_json::json!({
                "schema": PROVENANCE_SCHEMA_VERSION,
                "stage": "translation",
                "transcript": &transcript,
                "target_language": &job.target_language,
                "tone": &options.tone,
                "provider": self.translator.cache_identity(),
            }))
            .map_err(|e| {
                DomainError::Internal(format!(
                    "Failed to fingerprint translation provenance: {}",
                    e
                ))
            })?;

            let translated_path = artifact_store
                .verify_with_provenance(
                    "translation",
                    &artifact_manifest,
                    Some(&translation_provenance),
                )?
                .ok_or_else(|| {
                    DomainError::Internal(
                        "Translation artifact is missing, stale, or failed provenance/checksum validation"
                            .to_string(),
                    )
                })?;
            let content = std::fs::read_to_string(&translated_path).map_err(|e| {
                DomainError::Internal(format!("Failed to load translated doc: {}", e))
            })?;
            let parsed = serde_json::from_str(&content).map_err(|e| {
                DomainError::Internal(format!("Failed to parse translated doc: {}", e))
            })?;

            if !artifact_manifest.artifacts.contains_key("translation") {
                artifact_store.register_with_provenance(
                    "translation",
                    &translated_path,
                    &mut artifact_manifest,
                    Some(translation_provenance),
                )?;
                artifact_store.save(&artifact_manifest)?;
            }

            parsed
        };
        let t_translate = t_translate_start.elapsed();

        QualityGate::validate_translation(&transcript, &translated)?;

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
                            QualityGate::validate_translation(&transcript, &translated)?;
                            let data = serde_json::to_string_pretty(&translated).map_err(|e| {
                                DomainError::Internal(format!(
                                    "Serialize reviewed translation: {}",
                                    e
                                ))
                            })?;
                            write_atomic(&translated_path, data.as_bytes())?;
                            let translation_provenance = fingerprint(&serde_json::json!({
                                "schema": PROVENANCE_SCHEMA_VERSION,
                                "stage": "translation",
                                "transcript": &transcript,
                                "target_language": &job.target_language,
                                "tone": &options.tone,
                                "provider": self.translator.cache_identity(),
                            }))
                            .map_err(|e| {
                                DomainError::Internal(format!(
                                    "Failed to fingerprint reviewed translation provenance: {}",
                                    e
                                ))
                            })?;
                            artifact_store.register_with_provenance(
                                "translation",
                                &translated_path,
                                &mut artifact_manifest,
                                Some(translation_provenance),
                            )?;
                            ArtifactStore::invalidate_prefix(&mut artifact_manifest, "synthesis/");
                            ArtifactStore::invalidate_prefix(&mut artifact_manifest, "alignment/");
                            artifact_store.save(&artifact_manifest)?;
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

        QualityGate::validate_translation(&transcript, &translated)?;

        if let Err(e) = check_cancellation(&mut job) {
            self.job_repo.save(&job).await?;
            on_progress(&job);
            return Err(e);
        }

        // 4. Speech Synthesis Stage (Utterance Chunking with Speaker Awareness)
        // Provenance is part of cache identity so configuration changes regenerate audio.
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
        let synthesis_manifest = artifact_manifest.clone();
        let artifact_store_for_tasks = artifact_store.clone();
        let synthesizer_identity = self.synthesizer.cache_identity();
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
            let synthesis_manifest_for_task = synthesis_manifest.clone();
            let artifact_store_for_task = artifact_store_for_tasks.clone();
            let tts_provenance = fingerprint(&serde_json::json!({
                "schema": PROVENANCE_SCHEMA_VERSION,
                "stage": "synthesis",
                "segment": &seg,
                "voice": &voice,
                "provider": &synthesizer_identity,
            }))
            .map_err(|e| {
                DomainError::Internal(format!("Failed to fingerprint TTS provenance: {}", e))
            })?;

            tasks.push(async move {
                if cancel.is_cancelled() {
                    return Err(DomainError::Cancelled);
                }

                let tts_key = format!("synthesis/{idx:04}");
                let has_manifest_record =
                    synthesis_manifest_for_task.artifacts.contains_key(&tts_key);
                let cached_path = artifact_store_for_task
                    .verify_with_provenance(
                        &tts_key,
                        &synthesis_manifest_for_task,
                        Some(&tts_provenance),
                    )
                    .ok()
                    .flatten()
                    .or_else(|| {
                        // One-time migration for pre-provenance jobs. A legacy
                        // file is accepted only when no manifest record exists;
                        // after validation it is registered with current
                        // provenance so future config/model changes invalidate it.
                        if !has_manifest_record && segment_output_file.is_file() {
                            Some(segment_output_file.clone())
                        } else {
                            None
                        }
                    });

                if let Some(candidate) = cached_path {
                    if let Ok(meta) = std::fs::metadata(&candidate) {
                        if meta.len() > 1024 {
                            if let Ok(probe_meta) = engine.probe(&candidate).await {
                                if probe_meta.duration_ms > 0 {
                                    return Ok((
                                        idx,
                                        SynthesizedSegment {
                                            segment_id: seg.segment_id,
                                            speaker_id: seg.speaker_id,
                                            path: candidate,
                                            duration_ms: probe_meta.duration_ms,
                                        },
                                        tts_provenance,
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
                if idx > 0 && self.tts_request_spacing_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        self.tts_request_spacing_ms,
                    ))
                    .await;
                }

                let synth_result = synth
                    .synthesize_segment(&seg, &voice, &segment_output_file)
                    .await?;

                Ok((idx, synth_result, tts_provenance))
            });
        }

        // Controlled concurrency keeps API pressure and local memory usage bounded.
        let mut stream = stream::iter(tasks).buffer_unordered(self.tts_concurrency);
        let mut collected: Vec<(usize, SynthesizedSegment, String)> =
            Vec::with_capacity(translated.segments.len());

        while let Some(res) = stream.next().await {
            let (idx, synth_result, tts_provenance) = match res {
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

            artifact_store.register_with_provenance(
                format!("synthesis/{idx:04}"),
                &synth_result.path,
                &mut artifact_manifest,
                Some(tts_provenance.clone()),
            )?;
            collected.push((idx, synth_result, tts_provenance));
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
                artifact_store.save(&artifact_manifest)?;
                self.job_repo.save(&job).await?;
            }
            on_progress(&job);
        }

        collected.sort_by_key(|(idx, _, _)| *idx);
        let synthesized_segments: Vec<SynthesizedSegment> =
            collected.into_iter().map(|(_, seg, _)| seg).collect();
        QualityGate::validate_synthesis(&translated, &synthesized_segments)?;
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

        for (idx, path) in alignment_res.aligned_files.iter().enumerate() {
            artifact_store.register(format!("alignment/{idx:04}"), path, &mut artifact_manifest)?;
        }
        artifact_store.save(&artifact_manifest)?;
        QualityGate::validate_alignment(&synthesized_segments, &alignment_res)?;
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

        // Keep each job in its own output directory. This prevents two source files
        // with the same basename (or repeated runs) from overwriting each other.
        let output_dir = AppPaths::outputs_dir().join(job.id.as_str());
        std::fs::create_dir_all(&output_dir).map_err(|e| {
            DomainError::ExportError(format!("Failed to create output directory: {}", e))
        })?;
        let output_format = if self.audio_config.export_wav {
            AudioFormat::Wav
        } else {
            AudioFormat::Mp3
        };
        let output_file_name = format!(
            "{}_{}.{}",
            job.source_audio
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("audio"),
            job.target_language.as_str(),
            output_format.extension()
        );
        let final_output_path = output_dir.join(&output_file_name);

        let mut artifact = self
            .audio_engine
            .export_final(
                &alignment_res.aligned_files,
                &final_output_path,
                output_format,
                self.audio_config.default_bitrate_kbps,
                alignment_res.quality_warnings,
                Some(source_duration_ms),
            )
            .await?;

        let t_export = t_export_start.elapsed();

        // Subtitle export is opt-in. When enabled, failures are fatal rather than
        // silently producing a partial artifact set; writes are atomic so a crash
        // cannot leave a truncated subtitle file behind.
        let file_stem = job
            .source_audio
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio");

        if options.export_subtitles {
            let srt_path = output_dir.join(format!(
                "{}_{}.srt",
                file_stem,
                job.target_language.as_str()
            ));
            let vtt_path = output_dir.join(format!(
                "{}_{}.vtt",
                file_stem,
                job.target_language.as_str()
            ));
            let txt_path = output_dir.join(format!(
                "{}_{}_bilingual.txt",
                file_stem,
                job.target_language.as_str()
            ));

            let srt = generate_srt(&translated);
            write_atomic(&srt_path, srt.as_bytes())?;
            artifact.subtitle_srt_path = Some(srt_path);

            let vtt = generate_vtt(&translated);
            write_atomic(&vtt_path, vtt.as_bytes())?;
            artifact.subtitle_vtt_path = Some(vtt_path);

            let txt = generate_bilingual_txt(&translated);
            write_atomic(&txt_path, txt.as_bytes())?;
            artifact.transcript_txt_path = Some(txt_path);
        }

        // Audio Ducking (Smooth BGM attenuation)
        let mut audio_for_packaging = artifact.path.clone();
        if options.duck_audio {
            let ducked_output = output_dir.join(format!(
                "{}_{}_ducked.{}",
                file_stem,
                job.target_language.as_str(),
                artifact.format.extension()
            ));
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
            let remuxed_video_path = output_dir.join(format!(
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

        if let Err(err) = QualityGate::validate_output(&artifact) {
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
