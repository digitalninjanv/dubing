use crate::application::ports::AudioRouter;
use crate::domain::{
    AudioAppInfo, AudioSourceMode, DomainError, LanguageId, LiveDubberStatus, LiveModelChoice,
    LiveTranscriptUpdate,
};
use crate::infrastructure::audio_stream::{AudioStreamCapture, AudioStreamPlayer};
use crate::infrastructure::gemini::{GeminiLiveStreamer, LiveStreamSessionConfig};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Debug, Clone)]
pub struct LiveSessionOptions {
    pub model_choice: LiveModelChoice,
    pub source_mode: AudioSourceMode,
    /// Empty only for non-browser modes. Browser mode must resolve one or more browser streams
    /// before capture starts; silently capturing `default` is both unreliable and unsafe.
    pub target_app_ids: Vec<u32>,
    pub target_language: LanguageId,
    pub target_language_name: String,
    pub voice_name: String,
}

pub struct LiveDubberOrchestrator {
    audio_router: Arc<dyn AudioRouter>,
    live_streamer: Arc<GeminiLiveStreamer>,
    virtual_sink_name: String,
}

impl LiveDubberOrchestrator {
    pub fn new(audio_router: Arc<dyn AudioRouter>, live_streamer: Arc<GeminiLiveStreamer>) -> Self {
        Self {
            audio_router,
            live_streamer,
            virtual_sink_name: "AudioDub_Virtual_Sink".to_string(),
        }
    }

    /// Queries currently playing audio applications via the audio router.
    pub async fn list_audio_apps(&self) -> Result<Vec<AudioAppInfo>, DomainError> {
        self.audio_router.list_sink_inputs().await
    }

    /// Runs a full live dubbing session until cancelled.
    pub async fn start_session<FStatus, FTranscript>(
        &self,
        options: LiveSessionOptions,
        cancel_token: CancellationToken,
        on_status: FStatus,
        on_transcript: FTranscript,
    ) -> Result<(), DomainError>
    where
        FStatus: Fn(LiveDubberStatus) + Send + Sync + 'static,
        FTranscript: Fn(LiveTranscriptUpdate) + Send + Sync + 'static,
    {
        on_status(LiveDubberStatus::Initializing);

        let mut created_module_id: Option<u32> = None;
        let mut moved_apps: Vec<(u32, String)> = Vec::new();
        let capture_source: String;

        match options.source_mode {
            AudioSourceMode::BrowserYouTube => {
                on_status(LiveDubberStatus::RoutingAudio);
                info!("Setting up null sink to silence original YouTube/browser audio...");

                if options.target_app_ids.is_empty() {
                    return Err(DomainError::Internal(
                        "No active browser audio stream was found. Start browser playback, refresh the application list, then retry.".to_string(),
                    ));
                }

                if self.audio_router.is_available().await {
                    match self
                        .audio_router
                        .create_null_sink(&self.virtual_sink_name)
                        .await
                    {
                        Ok(mod_id) => {
                            created_module_id = Some(mod_id);
                            capture_source = format!("{}.monitor", self.virtual_sink_name);

                            for app_id in &options.target_app_ids {
                                match self
                                    .audio_router
                                    .move_sink_input(*app_id, &self.virtual_sink_name)
                                    .await
                                {
                                    Ok(original_sink) => {
                                        moved_apps.push((*app_id, original_sink));
                                        info!(
                                            "Target app #{} routed to isolated virtual sink",
                                            app_id
                                        );
                                    }
                                    Err(e) => {
                                        for (moved_id, original_sink) in &moved_apps {
                                            let _ = self
                                                .audio_router
                                                .restore_sink_input(*moved_id, original_sink)
                                                .await;
                                        }
                                        let _ = self.audio_router.unload_null_sink(mod_id).await;
                                        return Err(DomainError::Internal(format!(
                                            "Could not route selected browser audio stream #{}: {}",
                                            app_id, e
                                        )));
                                    }
                                }
                            }
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    return Err(DomainError::Internal(
                        "PulseAudio or PipeWire-Pulse is unavailable; browser audio routing cannot start.".to_string(),
                    ));
                }
            }
            AudioSourceMode::SystemDesktop => {
                capture_source = self.audio_router.get_default_monitor_source().await?;
            }
            AudioSourceMode::Microphone => {
                capture_source = self.audio_router.get_default_source_name().await?;
            }
        }

        let source_available = self.audio_router.source_exists(&capture_source).await;
        if !matches!(&source_available, Ok(true)) {
            for (app_id, original_sink) in &moved_apps {
                let _ = self
                    .audio_router
                    .restore_sink_input(*app_id, original_sink)
                    .await;
            }
            if let Some(mod_id) = created_module_id {
                let _ = self.audio_router.unload_null_sink(mod_id).await;
            }
            return match source_available {
                Ok(false) => Err(DomainError::Internal(format!(
                    "Audio source '{}' is not available. Verify the selected Linux audio device and start playback before retrying.",
                    capture_source
                ))),
                Err(error) => Err(error),
                Ok(true) => unreachable!("matched above"),
            };
        }

        // Setup streaming channels
        let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        let (transcript_tx, mut transcript_rx) =
            tokio::sync::mpsc::channel::<LiveTranscriptUpdate>(32);

        // 1. Start audio capture worker
        info!("Starting live capture worker on: {}", capture_source);
        AudioStreamCapture::start_capture(&capture_source, input_tx, cancel_token.clone()).await?;

        // 2. Start audio playback worker
        if options.model_choice.is_audio_output() {
            let target_sink = self
                .audio_router
                .get_default_sink_name()
                .await
                .unwrap_or_else(|_| "@DEFAULT_SINK@".to_string());
            info!("Starting live playback worker on: {}", target_sink);
            AudioStreamPlayer::start_playback(&target_sink, output_rx, cancel_token.clone())
                .await?;
        }

        // 3. Spawn transcript listener
        tokio::spawn(async move {
            while let Some(update) = transcript_rx.recv().await {
                on_transcript(update);
            }
        });

        on_status(LiveDubberStatus::Streaming);

        // 4. Run bidirectional Gemini Live API session
        let session_config = LiveStreamSessionConfig {
            model_choice: options.model_choice,
            target_lang: &options.target_language,
            target_lang_name: &options.target_language_name,
            voice_name: &options.voice_name,
        };

        let session_result = self
            .live_streamer
            .run_live_session(
                session_config,
                input_rx,
                output_tx,
                transcript_tx,
                cancel_token.clone(),
            )
            .await;

        // 5. Cleanup and teardown: restore original app audio routing
        for (app_id, original_sink) in moved_apps {
            info!(
                "Restoring target app #{} to its original audio sink...",
                app_id
            );
            let _ = self
                .audio_router
                .restore_sink_input(app_id, &original_sink)
                .await;
        }

        if let Some(mod_id) = created_module_id {
            info!("Unloading virtual null sink module {}...", mod_id);
            let _ = self.audio_router.unload_null_sink(mod_id).await;
        }

        on_status(LiveDubberStatus::Stopped);

        match session_result {
            Ok(()) => Ok(()),
            Err(DomainError::Cancelled) => Ok(()),
            Err(e) => {
                error!("Live Dubber session encountered error: {}", e);
                on_status(LiveDubberStatus::Error(e.to_string()));
                Err(e)
            }
        }
    }
}
