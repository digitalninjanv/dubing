use crate::domain::DomainError;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

pub struct AudioStreamCapture;

impl AudioStreamCapture {
    /// Capture 16 kHz signed-16 mono PCM for the Live API.
    /// PipeWire's native `pw-cat` is preferred; FFmpeg/PulseAudio is the compatibility fallback.
    pub async fn start_capture(
        source_name: &str,
        chunk_tx: Sender<Vec<u8>>,
        cancel_token: CancellationToken,
    ) -> Result<(), DomainError> {
        let is_default =
            source_name.is_empty() || source_name == "@DEFAULT_SOURCE@" || source_name == "default";
        let effective_source = if is_default { "default" } else { source_name };

        let use_pw_cat = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Command::new("pw-cat").arg("--version").output(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|o| o.status.success())
        .unwrap_or(false);

        let backend = if use_pw_cat {
            "PipeWire"
        } else {
            "PulseAudio/pipewire-pulse fallback"
        };
        info!(
            "Starting live capture from '{}' using {} backend",
            effective_source, backend
        );

        let mut child = if use_pw_cat {
            let mut cmd = Command::new("pw-cat");
            cmd.args([
                "--record",
                "--raw",
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "s16",
                "--latency",
                "20ms",
            ]);
            if !is_default {
                cmd.args(["--target", effective_source]);
            }
            cmd.arg("-")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| {
                    DomainError::Internal(format!(
                        "Failed to spawn PipeWire capture (pw-cat): {}",
                        e
                    ))
                })?
        } else {
            Command::new("ffmpeg")
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-fflags",
                    "nobuffer",
                    "-flags",
                    "low_delay",
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
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| {
                    DomainError::Internal(format!("Failed to spawn FFmpeg capture fallback: {}", e))
                })?
        };

        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        if let Ok(Some(status)) = child.try_wait() {
            let mut err_msg = String::new();
            if let Some(mut err_pipe) = child.stderr.take() {
                let mut err_buf = Vec::new();
                let _ = err_pipe.read_to_end(&mut err_buf).await;
                err_msg = String::from_utf8_lossy(&err_buf).trim().to_string();
            }
            return Err(DomainError::Internal(format!(
                "Live audio capture source '{}' unavailable via {} (status {}): {}",
                effective_source, backend, status, err_msg
            )));
        }

        let mut stdout = child.stdout.take().ok_or_else(|| {
            DomainError::Internal("Failed to capture live audio stdout".to_string())
        })?;
        let mut stderr = child.stderr.take();

        tokio::spawn(async move {
            const CHUNK_SIZE: usize = 3200;
            let mut buf = vec![0u8; CHUNK_SIZE];
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        debug!("Capture loop cancelled");
                        break;
                    }
                    read_res = stdout.read_exact(&mut buf) => {
                        match read_res {
                            Ok(_) => {
                                // Move buffer without clone (F6): take filled buf and
                                // replace with a fresh allocation.
                                let filled = std::mem::replace(&mut buf, vec![0u8; CHUNK_SIZE]);
                                if chunk_tx.send(filled).await.is_err() {
                                    debug!("Capture consumer closed");
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
                                warn!("Live capture stream ended: {} ({})", e, err_msg);
                                break;
                            }
                        }
                    }
                }
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            debug!("Live capture process terminated");
        });
        Ok(())
    }
}
