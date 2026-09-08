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
        let effective_sink = if sink_name.is_empty() || sink_name == "@DEFAULT_SINK@" {
            "default"
        } else {
            sink_name
        };

        info!("Starting live audio playback to sink: {}", effective_sink);

        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-fflags",
                "nobuffer",
                "-flags",
                "low_delay",
                "-probesize",
                "32",
                "-analyzeduration",
                "0",
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
                effective_sink,
            ])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| DomainError::Internal(format!("Failed to spawn ffmpeg audio player: {}", e)))?;

        // Fast health check: detect if ffmpeg died immediately on opening sink
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        if let Ok(Some(status)) = child.try_wait() {
            let mut err_msg = String::new();
            if let Some(mut err_pipe) = child.stderr.take() {
                use tokio::io::AsyncReadExt;
                let mut err_buf = Vec::new();
                let _ = err_pipe.read_to_end(&mut err_buf).await;
                err_msg = String::from_utf8_lossy(&err_buf).trim().to_string();
            }
            return Err(DomainError::Internal(format!(
                "Live audio playback sink '{}' unavailable (status {}): {}",
                effective_sink, status, err_msg
            )));
        }

        let mut stdin = child.stdin.take().ok_or_else(|| {
            DomainError::Internal("Failed to capture stdin of ffmpeg player".to_string())
        })?;
        let mut stderr = child.stderr.take();

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
                                    let mut err_msg = String::new();
                                    if let Some(mut err_pipe) = stderr.take() {
                                        use tokio::io::AsyncReadExt;
                                        let mut err_buf = Vec::new();
                                        let _ = err_pipe.read_to_end(&mut err_buf).await;
                                        err_msg = String::from_utf8_lossy(&err_buf).trim().to_string();
                                    }
                                    warn!(
                                        "Error writing PCM chunk to audio playback: {} (ffmpeg details: {})",
                                        e, err_msg
                                    );
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
