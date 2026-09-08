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
        let effective_source = if source_name.is_empty() || source_name == "@DEFAULT_SOURCE@" {
            "default"
        } else {
            source_name
        };

        info!("Starting live audio capture from source: {}", effective_source);

        // Low-latency FFmpeg pulse capture to stdout
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
                "pulse",
                "-i",
                effective_source,
                "-vn",
                "-f",
                "s16le",
                "-acodec",
                "pcm_s16le",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-flush_packets",
                "1",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| DomainError::Internal(format!("Failed to spawn ffmpeg capture: {}", e)))?;

        // Fast health check: detect if ffmpeg died immediately on opening input
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        if let Ok(Some(status)) = child.try_wait() {
            let mut err_msg = String::new();
            if let Some(mut err_pipe) = child.stderr.take() {
                let mut err_buf = Vec::new();
                let _ = err_pipe.read_to_end(&mut err_buf).await;
                err_msg = String::from_utf8_lossy(&err_buf).trim().to_string();
            }
            return Err(DomainError::Internal(format!(
                "Live audio capture source '{}' unavailable (status {}): {}",
                effective_source, status, err_msg
            )));
        }

        let mut stdout = child.stdout.take().ok_or_else(|| {
            DomainError::Internal("Failed to capture stdout of ffmpeg".to_string())
        })?;
        let mut stderr = child.stderr.take();

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
                                let mut err_msg = String::new();
                                if let Some(mut err_pipe) = stderr.take() {
                                    let mut err_buf = Vec::new();
                                    let _ = err_pipe.read_to_end(&mut err_buf).await;
                                    err_msg = String::from_utf8_lossy(&err_buf).trim().to_string();
                                }
                                warn!(
                                    "Capture read error or process terminated: {} (ffmpeg details: {})",
                                    e, err_msg
                                );
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
