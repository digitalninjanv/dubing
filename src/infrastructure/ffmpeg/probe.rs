use crate::domain::{DomainError, MediaMetadata};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_name: Option<String>,
    codec_type: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Option<Vec<FfprobeStream>>,
    format: Option<FfprobeFormat>,
}

pub struct FfprobeInspector;

impl FfprobeInspector {
    pub fn probe(path: &Path) -> Result<MediaMetadata, DomainError> {
        if !path.exists() {
            return Err(DomainError::InvalidAudio(format!(
                "File does not exist: {}",
                path.display()
            )));
        }

        // Run ffprobe structured without shell interpolation
        // Include codec_type so we can prefer audio streams (F11).
        let output = Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("format=duration,bit_rate:stream=codec_name,codec_type,sample_rate,channels")
            .arg("-of")
            .arg("json")
            .arg(path)
            .output()
            .map_err(|e| {
                DomainError::Internal(format!(
                    "Failed to execute ffprobe. Ensure ffmpeg is installed. Details: {}",
                    e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DomainError::InvalidAudio(format!(
                "ffprobe failed to inspect file: {}",
                stderr
            )));
        }

        let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout).map_err(|e| {
            DomainError::InvalidAudio(format!("Failed to parse ffprobe json output: {}", e))
        })?;

        // F11: prefer audio stream; fallback to first stream if none tagged.
        let stream = parsed.streams.and_then(|mut s| {
            if s.is_empty() {
                return None;
            }
            // Prefer streams with codec_type == "audio" and highest sample_rate.
            let mut best_idx: Option<usize> = None;
            let mut best_rate: u32 = 0;
            for (i, st) in s.iter().enumerate() {
                if st.codec_type.as_deref() == Some("audio") {
                    let rate = st
                        .sample_rate
                        .as_deref()
                        .and_then(|r| r.parse::<u32>().ok())
                        .unwrap_or(0);
                    if best_idx.is_none() || rate > best_rate {
                        best_idx = Some(i);
                        best_rate = rate;
                    }
                }
            }
            if let Some(idx) = best_idx {
                Some(s.remove(idx))
            } else {
                Some(s.remove(0))
            }
        });

        let duration_secs: f64 = parsed
            .format
            .as_ref()
            .and_then(|f| f.duration.as_ref())
            .and_then(|d| d.parse::<f64>().ok())
            .unwrap_or(0.0);

        let duration_ms = (duration_secs * 1000.0).round() as u64;
        if duration_ms == 0 {
            return Err(DomainError::InvalidAudio(
                "Audio duration is 0 or cannot be determined".to_string(),
            ));
        }

        let sample_rate = stream
            .as_ref()
            .and_then(|s| s.sample_rate.as_ref())
            .and_then(|sr| sr.parse::<u32>().ok())
            .unwrap_or(44100);

        let channels = stream.as_ref().and_then(|s| s.channels).unwrap_or(2);

        let codec = stream
            .as_ref()
            .and_then(|s| s.codec_name.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let bitrate = parsed
            .format
            .as_ref()
            .and_then(|f| f.bit_rate.as_ref())
            .and_then(|br| br.parse::<u64>().ok());

        Ok(MediaMetadata {
            duration_ms,
            sample_rate,
            channels,
            codec,
            bitrate,
        })
    }
}
