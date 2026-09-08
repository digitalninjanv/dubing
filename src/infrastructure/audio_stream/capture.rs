use crate::domain::DomainError;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

pub struct AudioStreamCapture;

impl AudioStreamCapture {
    /// Spawns an asynchronous capture loop reading raw 16kHz 16-bit mono PCM from a PulseAudio/PipeWire source.
    /// Emits 100ms chunks (3,200 bytes) into the provided `chunk_tx`.
    pub async fn start_capture(
        source_name: &str,
        chunk_tx: Sender<Vec<u8>>,
        cancel_token: CancellationToken,
    ) -> Result<(), DomainError> {
        info!("Starting live audio capture from source: {}", source_name);

        // Standard FFmpeg pulse capture to stdout
        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "pulse",
                "-i",
                source_name,
                "-vn",
                "-f",
                "s16le",
                "-acodec",
                "pcm_s16le",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| DomainError::Internal(format!("Failed to spawn ffmpeg capture: {}", e)))?;

        let mut stdout = child.stdout.take().ok_or_else(|| {
            DomainError::Internal("Failed to capture stdout of ffmpeg".to_string())
        })?;

        tokio::spawn(async move {
            const CHUNK_SIZE: usize = 3200; // 100ms at 16kHz 16-bit mono
            let mut buf = vec![0u8; CHUNK_SIZE];

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        debug!("Capture loop received cancellation signal");
                        break;
                    }
                    read_res = stdout.read_exact(&mut buf) => {
                        match read_res {
                            Ok(_) => {
                                if chunk_tx.send(buf.clone()).await.is_err() {
                                    debug!("Capture consumer channel closed; exiting capture loop");
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Capture read error or end-of-stream: {}", e);
                                break;
                            }
                        }
                    }
                }
            }

            let _ = child.kill().await;
            debug!("Live capture process terminated");
        });

        Ok(())
    }
}
