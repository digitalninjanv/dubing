use crate::application::ports::AudioRouter;
use crate::domain::{
    AudioAppInfo, AudioSourceMode, DomainError, LanguageId, LiveDubberStatus, LiveModelChoice,
    LiveTranscriptUpdate,
};
use crate::infrastructure::audio_stream::{AudioStreamCapture, AudioStreamPlayer};
use crate::infrastructure::gemini::{GeminiLiveStreamer, LiveStreamSessionConfig};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
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
        let moved_apps: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));
        let capture_source: String;

        match options.source_mode {
            AudioSourceMode::BrowserYouTube => {
                on_status(LiveDubberStatus::RoutingAudio);
                info!("Setting up null sink to silence original YouTube/browser audio...");

                if self.audio_router.is_available().await {
                    match self.audio_router.create_null_sink(&self.virtual_sink_name).await {
                        Ok(mod_id) => {
                            created_module_id = Some(mod_id);
                            // PipeWire + pipewire-pulse needs longer settle time for
                            // module-null-sink monitor source to become available.
                            // 150ms was insufficient on many modern distros.
                            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                            capture_source = format!("{}.monitor", self.virtual_sink_name);

                            if let Some(app_id) = options.target_app_id {
                                if let Err(e) = self
                                    .audio_router
                                    .move_sink_input(app_id, &self.virtual_sink_name)
                                    .await
                                {
                                    warn!("Failed to move target app to virtual sink: {}", e);
                                } else {
                                    moved_apps.lock().await.insert(app_id);
                                    info!(
                                        "Target app #{} audio routed to virtual sink (original sound silenced from speakers)",
                                        app_id
                                    );
                                }
                            } else {
                                // Auto-detect: continuously watch and route all browser streams (YouTube, Chrome, Firefox, etc.)
                                let watcher_router = self.audio_router.clone();
                                let watcher_sink = self.virtual_sink_name.clone();
                                let watcher_moved = moved_apps.clone();
                                let watcher_token = cancel_token.clone();

                                tokio::spawn(async move {
                                    while !watcher_token.is_cancelled() {
                                        if let Ok(apps) = watcher_router.list_sink_inputs().await {
                                            for app in apps {
                                                if app.is_browser() {
                                                    let mut locked = watcher_moved.lock().await;
                                                    if !locked.contains(&app.sink_input_id) {
                                                        info!(
                                                            "Auto-detected browser audio stream '{}' (#{}); routing to virtual sink",
                                                            app.application_name, app.sink_input_id
                                                        );
                                                        if watcher_router
                                                            .move_sink_input(app.sink_input_id, &watcher_sink)
                                                            .await
                                                            .is_ok()
                                                        {
                                                            locked.insert(app.sink_input_id);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        tokio::select! {
                                            _ = watcher_token.cancelled() => break,
                                            _ = tokio::time::sleep(std::time::Duration::from_millis(700)) => {}
                                        }
                                    }
                                });
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
                if let Ok(sink) = self.audio_router.get_default_sink_name().await {
                    if !sink.is_empty() && sink != "default" {
                        capture_source = format!("{}.monitor", sink);
                    } else {
                        capture_source = "default".to_string();
                    }
                } else {
                    capture_source = "default".to_string();
                }
            }
            AudioSourceMode::Microphone => {
                capture_source = "default".to_string();
            }
        }

        let run_result = async {
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
                    .unwrap_or_else(|_| "default".to_string());
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

            self.live_streamer
                .run_live_session(
                    session_config,
                    input_rx,
                    output_tx,
                    transcript_tx,
                    cancel_token.clone(),
                )
                .await
        }
        .await;

        // 5. Cleanup and teardown: ALWAYS restore original app audio routing
        let to_restore: Vec<u32> = {
            let locked = moved_apps.lock().await;
            locked.iter().copied().collect()
        };

        for app_id in to_restore {
            info!("Restoring target app #{} audio routing...", app_id);
            let _ = self.audio_router.restore_sink_input(app_id).await;
        }

        if let Some(mod_id) = created_module_id {
            info!("Unloading virtual null sink module {}...", mod_id);
            let _ = self.audio_router.unload_null_sink(mod_id).await;
        }

        match run_result {
            Ok(()) => {
                on_status(LiveDubberStatus::Stopped);
                Ok(())
            }
            Err(DomainError::Cancelled) => {
                on_status(LiveDubberStatus::Stopped);
                Ok(())
            }
            Err(e) => {
                error!("Live Dubber session encountered error: {}", e);
                on_status(LiveDubberStatus::Error(e.to_string()));
                Err(e)
            }
        }
    }
}
