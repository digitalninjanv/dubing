use crate::domain::DomainError;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

pub struct AudioStreamPlayer;

impl AudioStreamPlayer {
    /// Play raw 24 kHz signed-16 mono PCM in realtime.
    /// PipeWire's native `pw-cat` is preferred; FFmpeg/PulseAudio remains the compatibility fallback.
    pub async fn start_playback(
        sink_name: &str,
        mut pcm_rx: Receiver<Vec<u8>>,
        mut flush_rx: Receiver<()>,
        cancel_token: CancellationToken,
    ) -> Result<(), DomainError> {
        let is_default =
            sink_name.is_empty() || sink_name == "@DEFAULT_SINK@" || sink_name == "default";
        let effective_sink = if is_default { "default" } else { sink_name };

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
            "Starting live playback to '{}' using {} backend",
            effective_sink, backend
        );

        let mut child = if use_pw_cat {
            let mut cmd = Command::new("pw-cat");
            cmd.args([
                "--playback",
                "--raw",
                "--rate",
                "24000",
                "--channels",
                "1",
                "--format",
                "s16",
                "--latency",
                "20ms",
            ]);
            if !is_default {
                cmd.args(["--target", effective_sink]);
            }
            cmd.arg("-")
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| {
                    DomainError::Internal(format!(
                        "Failed to spawn PipeWire playback (pw-cat): {}",
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
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| {
                    DomainError::Internal(format!(
                        "Failed to spawn FFmpeg playback fallback: {}",
                        e
                    ))
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
                "Live playback sink '{}' unavailable via {} (status {}): {}",
                effective_sink, backend, status, err_msg
            )));
        }

        let mut stdin = child.stdin.take().ok_or_else(|| {
            DomainError::Internal("Failed to capture live playback stdin".to_string())
        })?;
        let mut stderr = child.stderr.take();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        debug!("Playback loop cancelled");
                        break;
                    }
                    // Server signalled `interrupted`: drop stale queued audio
                    // so playback doesn't lag behind the live conversation.
                    _ = flush_rx.recv() => {
                        let mut dropped = 0u32;
                        while pcm_rx.try_recv().is_ok() {
                            dropped += 1;
                        }
                        debug!("Playback queue flushed ({dropped} stale chunks dropped)");
                    }
                    maybe_chunk = pcm_rx.recv() => {
                        match maybe_chunk {
                            Some(chunk) => {
                                if let Err(e) = stdin.write_all(&chunk).await {
                                    let mut err_msg = String::new();
                                    if let Some(mut err_pipe) = stderr.take() {
                                        let mut err_buf = Vec::new();
                                        let _ = err_pipe.read_to_end(&mut err_buf).await;
                                        err_msg = String::from_utf8_lossy(&err_buf).trim().to_string();
                                    }
                                    warn!("Live playback stream ended: {} ({})", e, err_msg);
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            debug!("Live playback process terminated");
        });
        Ok(())
    }
}
