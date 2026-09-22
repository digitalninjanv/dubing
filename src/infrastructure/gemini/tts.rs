use super::client::GeminiClient;
use crate::application::ports::SpeechSynthesizer;
use crate::domain::{DomainError, SynthesizedSegment, TranslationSegment, VoiceProfile};
use crate::infrastructure::ffmpeg::FfprobeInspector;
use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct TtsCandidatePart {
    #[serde(rename = "inlineData")]
    inline_data: Option<InlineAudioData>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct InlineAudioData {
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TtsCandidate {
    content: Option<TtsCandidateContent>,
}

#[derive(Debug, Deserialize)]
struct TtsCandidateContent {
    parts: Option<Vec<TtsCandidatePart>>,
}

#[derive(Debug, Deserialize)]
struct TtsResponse {
    candidates: Option<Vec<TtsCandidate>>,
    /// Interactions API (`POST /v1beta/interactions`) returns decoded audio
    /// under `output_audio.data` (SDK spelling `outputAudio` also tolerated).
    #[serde(rename = "outputAudio")]
    output_audio_camel: Option<InteractionsAudio>,
    #[serde(rename = "output_audio")]
    output_audio_snake: Option<InteractionsAudio>,
}

#[derive(Debug, Deserialize)]
struct InteractionsAudio {
    data: Option<String>,
}

pub struct GeminiSynthesizer {
    client: GeminiClient,
    model_name: String,
}

impl GeminiSynthesizer {
    pub fn new(client: GeminiClient, model_name: impl Into<String>) -> Self {
        Self {
            client,
            model_name: model_name.into(),
        }
    }

    /// Helper to wrap raw 24kHz 16-bit mono PCM into a standard RIFF WAV container if needed
    fn wrap_pcm_to_wav(pcm_data: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
        let byte_rate = sample_rate * (channels as u32) * 2;
        let block_align = channels * 2;
        let data_len = pcm_data.len() as u32;
        let file_len = 36 + data_len;

        let mut wav = Vec::with_capacity(pcm_data.len() + 44);
        // RIFF header
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&file_len.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        // fmt subchunk
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (16 for PCM)
        wav.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat (1 for PCM)
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample (16)
                                                     // data subchunk
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(pcm_data);

        wav
    }
    /// Helper to execute TTS request against a specific Gemini model via the
    /// Interactions API (`POST /v1beta/interactions`) — the current documented
    /// TTS endpoint (ai.google.dev/gemini-api/docs speech-generation).
    /// Returns the raw audio bytes (PCM 24kHz mono or WAV).
    async fn try_interactions_with_model(
        &self,
        model: &str,
        segment: &TranslationSegment,
        voice: &VoiceProfile,
    ) -> Result<Vec<u8>, DomainError> {
        let endpoint = format!("{}/v1beta/interactions", self.client.base_url(),);

        let prompt = match &voice.style {
            Some(style) if !style.trim().is_empty() => {
                format!("{}\n{}", style.trim(), segment.translated_text)
            }
            _ => segment.translated_text.clone(),
        };

        let request_body = json!({
            "model": model,
            "input": prompt,
            "response_format": { "type": "audio" },
            "generation_config": {
                "speech_config": [
                    { "voice": voice.voice_name }
                ]
            }
        });

        let body_bytes = serde_json::to_vec(&request_body).map_err(|e| {
            DomainError::Internal(format!("Failed to serialize TTS request: {}", e))
        })?;

        let http_client = self.client.http().clone();
        let target_endpoint = endpoint.clone();
        let op_name = format!("Gemini TTS interactions ({})", model);
        let api_key = self.client.api_key().to_string();

        let response = self
            .client
            .post_with_retry(&op_name, || {
                let cli = http_client.clone();
                let url = target_endpoint.clone();
                let bytes = body_bytes.clone();
                let key = api_key.clone();
                async move {
                    cli.post(&url)
                        .header("Content-Type", "application/json")
                        .header("x-goog-api-key", key)
                        .body(bytes)
                        .send()
                        .await
                }
            })
            .await?;

        let parsed: TtsResponse = response.json().await.map_err(|e| {
            DomainError::PermanentApiError(format!("Failed to parse TTS response: {}", e))
        })?;

        let base64_str = parsed
            .output_audio_snake
            .or(parsed.output_audio_camel)
            .and_then(|a| a.data)
            .ok_or_else(|| {
                DomainError::PermanentApiError(
                    "No output_audio data returned from Gemini TTS interactions".to_string(),
                )
            })?;

        base64::engine::general_purpose::STANDARD
            .decode(base64_str.trim())
            .map_err(|e| {
                DomainError::PermanentApiError(format!(
                    "Failed to decode base64 audio bytes: {}",
                    e
                ))
            })
    }

    /// Helper to execute TTS request against a specific Gemini model
    async fn try_synthesize_with_model(
        &self,
        model: &str,
        segment: &TranslationSegment,
        voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        let endpoint = format!(
            "{}/v1beta/models/{}:generateContent",
            self.client.base_url(),
            model,
        );

        let duration_hint = if segment.target_duration_ms() > 0 {
            let sec = segment.target_duration_ms() as f64 / 1000.0;
            format!("Pacing instruction: Speak briskly and clearly within approximately {:.1} seconds without unnatural pauses or dragging vowels.\n", sec)
        } else {
            String::new()
        };

        let prompt = match &voice.style {
            Some(style) if !style.trim().is_empty() => {
                format!(
                    "Speaking style instructions: {}\n{}Text to synthesize:\n{}",
                    style.trim(),
                    duration_hint,
                    segment.translated_text
                )
            }
            _ => {
                format!(
                    "Speaking style instructions: Professional audio dubbing. {}Text to synthesize:\n{}",
                    duration_hint,
                    segment.translated_text
                )
            }
        };

        let request_body = json!({
            "contents": [
                {
                    "parts": [
                        { "text": prompt }
                    ]
                }
            ],
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "speechConfig": {
                    "voiceConfig": {
                        "prebuiltVoiceConfig": {
                            "voiceName": voice.voice_name
                        }
                    }
                }
            }
        });

        let body_bytes = serde_json::to_vec(&request_body).map_err(|e| {
            DomainError::Internal(format!("Failed to serialize TTS request: {}", e))
        })?;

        let http_client = self.client.http().clone();
        let target_endpoint = endpoint.clone();
        let op_name = format!("Gemini TTS ({})", model);
        let api_key = self.client.api_key().to_string();

        let response = self
            .client
            .post_with_retry(&op_name, || {
                let cli = http_client.clone();
                let url = target_endpoint.clone();
                let bytes = body_bytes.clone();
                let key = api_key.clone();
                async move {
                    cli.post(&url)
                        .header("Content-Type", "application/json")
                        .header("x-goog-api-key", key)
                        .body(bytes)
                        .send()
                        .await
                }
            })
            .await?;

        let parsed: TtsResponse = response.json().await.map_err(|e| {
            DomainError::PermanentApiError(format!("Failed to parse TTS response: {}", e))
        })?;

        let inline_data = parsed
            .candidates
            .and_then(|mut c| {
                if !c.is_empty() {
                    Some(c.remove(0))
                } else {
                    None
                }
            })
            .and_then(|c| c.content)
            .and_then(|cnt| cnt.parts)
            .and_then(|mut p| {
                if !p.is_empty() {
                    Some(p.remove(0))
                } else {
                    None
                }
            })
            .and_then(|p| p.inline_data)
            .ok_or_else(|| {
                DomainError::PermanentApiError(
                    "No inline audio data returned from Gemini TTS".to_string(),
                )
            })?;

        let base64_str = inline_data.data.ok_or_else(|| {
            DomainError::PermanentApiError("Inline audio data is empty".to_string())
        })?;

        let audio_bytes = base64::engine::general_purpose::STANDARD
            .decode(base64_str.trim())
            .map_err(|e| {
                DomainError::PermanentApiError(format!(
                    "Failed to decode base64 audio bytes: {}",
                    e
                ))
            })?;

        // If audio data is raw PCM without RIFF header, wrap it in a standard WAV header
        let output_bytes = if !audio_bytes.starts_with(b"RIFF") && !audio_bytes.starts_with(b"ID3")
        {
            Self::wrap_pcm_to_wav(&audio_bytes, 24000, 1)
        } else {
            audio_bytes
        };

        self.persist_segment_bytes(output_path, output_bytes, segment)
            .await
    }

    /// Read duration directly from a canonical PCM WAV buffer.
    ///
    /// Generated Gemini TTS audio is wrapped as 24 kHz, 16-bit mono PCM WAV.
    /// Parsing its RIFF container avoids spawning ffprobe for every segment.
    fn wav_duration_ms(data: &[u8]) -> Option<u64> {
        if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
            return None;
        }

        let mut offset = 12usize;
        let mut byte_rate = None::<u32>;

        while offset + 8 <= data.len() {
            let chunk_id = &data[offset..offset + 4];
            let chunk_size = u32::from_le_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]) as usize;
            let chunk_start = offset + 8;
            let chunk_end = chunk_start.saturating_add(chunk_size);

            if chunk_end > data.len() {
                return None;
            }

            match chunk_id {
                b"fmt " if chunk_size >= 16 => {
                    let channels =
                        u16::from_le_bytes([data[chunk_start + 2], data[chunk_start + 3]]);
                    let sample_rate = u32::from_le_bytes([
                        data[chunk_start + 4],
                        data[chunk_start + 5],
                        data[chunk_start + 6],
                        data[chunk_start + 7],
                    ]);
                    let bits_per_sample =
                        u16::from_le_bytes([data[chunk_start + 14], data[chunk_start + 15]]);

                    if channels == 0 || sample_rate == 0 || bits_per_sample < 8 {
                        return None;
                    }

                    let bytes_per_sample = (bits_per_sample / 8) as u32;
                    if bytes_per_sample == 0 {
                        return None;
                    }

                    byte_rate = Some(
                        sample_rate
                            .saturating_mul(channels as u32)
                            .saturating_mul(bytes_per_sample),
                    );
                }
                b"data" => {
                    if let Some(rate) = byte_rate {
                        if rate > 0 {
                            return Some((chunk_size as u64 * 1000) / rate as u64);
                        }
                    }
                }
                _ => {}
            }

            offset = chunk_end + (chunk_size & 1);
        }

        None
    }

    /// Write decoded audio bytes to disk (wrapping raw PCM in WAV when needed)
    /// and probe the resulting duration.
    async fn persist_segment_bytes(
        &self,
        output_path: &Path,
        output_bytes: Vec<u8>,
        segment: &TranslationSegment,
    ) -> Result<SynthesizedSegment, DomainError> {
        let tmp_path = output_path.with_extension("wav.tmp");
        let mut file = File::create(&tmp_path).map_err(|e| {
            DomainError::Internal(format!("Failed to create temporary segment audio file: {}", e))
        })?;

        file.write_all(&output_bytes).map_err(|e| {
            DomainError::Internal(format!("Failed to write segment audio bytes: {}", e))
        })?;
        file.sync_all().map_err(|e| {
            DomainError::Internal(format!("Failed to flush segment audio file: {}", e))
        })?;
        drop(file);

        let duration_ms = if output_bytes.starts_with(b"RIFF") {
            Self::wav_duration_ms(&output_bytes).ok_or_else(|| {
                DomainError::PermanentApiError(
                    "Generated WAV audio has an invalid or unsupported header".to_string(),
                )
            })?
        } else {
            // Keep ffprobe only for non-WAV/legacy responses.
            let p = tmp_path.clone();
            let metadata = tokio::task::spawn_blocking(move || FfprobeInspector::probe(&p))
                .await
                .map_err(|e| DomainError::Internal(format!("Task error: {}", e)))??;
            metadata.duration_ms
        };

        if duration_ms == 0 {
            return Err(DomainError::PermanentApiError(
                "Generated TTS audio has zero duration".to_string(),
            ));
        }

        std::fs::rename(&tmp_path, output_path).map_err(|e| {
            DomainError::Internal(format!("Failed to publish segment audio file: {}", e))
        })?;

        Ok(SynthesizedSegment {
            segment_id: segment.segment_id.clone(),
            speaker_id: segment.speaker_id.clone(),
            path: output_path.to_path_buf(),
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GeminiSynthesizer;

    #[test]
    fn wav_duration_parser_handles_pcm_wav() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&36u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&24000u32.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.resize(44 + 48000, 0);

        assert_eq!(GeminiSynthesizer::wav_duration_ms(&wav), Some(1000));
    }

    #[test]
    fn wav_duration_parser_rejects_truncated_data() {
        assert_eq!(GeminiSynthesizer::wav_duration_ms(b"RIFF"), None);
    }
}

#[async_trait]
impl SpeechSynthesizer for GeminiSynthesizer {
    async fn synthesize_segment(
        &self,
        segment: &TranslationSegment,
        voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        let mut candidates = vec![self.model_name.as_str()];
        let fallbacks = ["gemini-2.5-flash-preview-tts", "gemini-2.5-pro-preview-tts"];
        for fb in fallbacks {
            if !candidates.contains(&fb) {
                candidates.push(fb);
            }
        }

        let mut last_err = None;
        for (i, &model) in candidates.iter().enumerate() {
            if i > 0 {
                info!(
                    "Fallback cascade: attempting TTS speech synthesis with model '{}'",
                    model
                );
                if let Some(cb) = self.client.status_callback() {
                    cb(&format!("Fallback to TTS model '{}'...", model));
                }
            }

            // Primary: Interactions API (documented TTS endpoint).
            match self
                .try_interactions_with_model(model, segment, voice)
                .await
            {
                Ok(raw_bytes) => {
                    let output_bytes =
                        if !raw_bytes.starts_with(b"RIFF") && !raw_bytes.starts_with(b"ID3") {
                            Self::wrap_pcm_to_wav(&raw_bytes, 24000, 1)
                        } else {
                            raw_bytes
                        };
                    match self
                        .persist_segment_bytes(output_path, output_bytes, segment)
                        .await
                    {
                        Ok(res) => return Ok(res),
                        Err(e) => {
                            warn!(
                                "TTS persist failed with model '{}': {}. Trying legacy endpoint...",
                                model, e
                            );
                        }
                    }
                }
                Err(err) => {
                    if matches!(
                        err,
                        DomainError::Cancelled | DomainError::AuthenticationFailed
                    ) {
                        return Err(err);
                    }
                    warn!(
                        "TTS interactions failed with model '{}': {}. Trying legacy generateContent...",
                        model, err
                    );
                }
            }

            // Fallback: legacy generateContent endpoint (kept while Google
            // keeps serving it).
            match self
                .try_synthesize_with_model(model, segment, voice, output_path)
                .await
            {
                Ok(res) => return Ok(res),
                Err(err) => {
                    warn!("TTS synthesis failed with model '{}': {}", model, err);
                    if matches!(
                        err,
                        DomainError::Cancelled | DomainError::AuthenticationFailed
                    ) {
                        return Err(err);
                    }
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            DomainError::PermanentApiError("All TTS model candidates failed".to_string())
        }))
    }

    async fn synthesize_text(
        &self,
        text: &str,
        voice: &VoiceProfile,
        style_instruction: Option<&str>,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        let dummy = TranslationSegment {
            segment_id: "direct_tts".to_string(),
            speaker_id: None,
            source_start_ms: 0,
            source_end_ms: 0,
            source_text: text.to_string(),
            translated_text: text.to_string(),
        };
        let mut custom_voice = voice.clone();
        if let Some(s) = style_instruction {
            custom_voice.style = Some(s.to_string());
        }
        self.synthesize_segment(&dummy, &custom_voice, output_path)
            .await
    }
}
