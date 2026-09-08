use crate::domain::DomainError;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

pub struct AudioStreamPlayer;

impl AudioStreamPlayer {
    /// Spawns an asynchronous playback worker that receives raw 24kHz 16-bit mono PCM chunks
    /// and streams them directly to the physical audio output sink via FFmpeg/PulseAudio.
    pub async fn start_playback(
        sink_name: &str,
        mut pcm_rx: Receiver<Vec<u8>>,
        cancel_token: CancellationToken,
    ) -> Result<(), DomainError> {
        info!("Starting live audio playback to sink: {}", sink_name);

        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "s16le",
                "-ar",
                "24000",
                "-ac",
                "1",
                "-i",
                "-",
                "-f",
                "pulse",
                sink_name,
            ])
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| DomainError::Internal(format!("Failed to spawn ffmpeg audio player: {}", e)))?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            DomainError::Internal("Failed to capture stdin of ffmpeg player".to_string())
        })?;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        debug!("Playback loop received cancellation signal");
                        break;
                    }
                    maybe_chunk = pcm_rx.recv() => {
                        match maybe_chunk {
                            Some(chunk) => {
                                if let Err(e) = stdin.write_all(&chunk).await {
                                    warn!("Error writing PCM chunk to audio playback stdin: {}", e);
                                    break;
                                }
                                let _ = stdin.flush().await;
                            }
                            None => {
                                debug!("Playback receiver channel closed");
                                break;
                            }
                        }
                    }
                }
            }

            let _ = child.kill().await;
            debug!("Live playback process terminated");
        });

        Ok(())
    }
}
