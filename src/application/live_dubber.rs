use crate::application::ports::AudioRouter;
use crate::domain::{
    AudioAppInfo, AudioSourceMode, DomainError, LanguageId, LiveDubberStatus, LiveModelChoice,
    LiveTranscriptUpdate,
};
use crate::infrastructure::audio_stream::{AudioStreamCapture, AudioStreamPlayer};
use crate::infrastructure::gemini::{GeminiLiveStreamer, LiveStreamSessionConfig};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct LiveSessionOptions {
    pub model_choice: LiveModelChoice,
    pub source_mode: AudioSourceMode,
    pub target_app_id: Option<u32>,
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
    pub fn new(
        audio_router: Arc<dyn AudioRouter>,
        live_streamer: Arc<GeminiLiveStreamer>,
    ) -> Self {
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
        let mut moved_app_id: Option<u32> = None;
        let capture_source: String;

        match options.source_mode {
            AudioSourceMode::BrowserYouTube => {
                on_status(LiveDubberStatus::RoutingAudio);
                info!("Setting up null sink to silence original YouTube/browser audio...");

                if self.audio_router.is_available().await {
                    match self.audio_router.create_null_sink(&self.virtual_sink_name).await {
                        Ok(mod_id) => {
                            created_module_id = Some(mod_id);
                            capture_source = format!("{}.monitor", self.virtual_sink_name);

                            if let Some(app_id) = options.target_app_id {
                                if let Err(e) = self
                                    .audio_router
                                    .move_sink_input(app_id, &self.virtual_sink_name)
                                    .await
                                {
                                    warn!("Failed to move target app to virtual sink: {}", e);
                                } else {
                                    moved_app_id = Some(app_id);
                                    info!(
                                        "Target app #{} audio routed to virtual sink (original sound silenced from speakers)",
                                        app_id
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Failed to create null sink ({}); falling back to default monitor",
                                e
                            );
                            capture_source = "default".to_string();
                        }
                    }
                } else {
                    warn!("Audio router not available on system; capturing default audio device");
                    capture_source = "default".to_string();
                }
            }
            AudioSourceMode::SystemDesktop => {
                capture_source = "default".to_string();
            }
            AudioSourceMode::Microphone => {
                capture_source = "default".to_string();
            }
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
            AudioStreamPlayer::start_playback(&target_sink, output_rx, cancel_token.clone()).await?;
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
        if let Some(app_id) = moved_app_id {
            info!("Restoring target app #{} audio routing...", app_id);
            let _ = self.audio_router.restore_sink_input(app_id).await;
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
                Err(e)
            }
        }
    }
}
