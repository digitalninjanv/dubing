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
}

#[async_trait]
impl SpeechSynthesizer for GeminiSynthesizer {
    async fn synthesize_segment(
        &self,
        segment: &TranslationSegment,
        voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        let endpoint = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.client.base_url(),
            self.model_name,
            self.client.api_key()
        );

        let prompt = format!(
            "Synthesize the following speech naturally and clearly:\n\n{}",
            segment.translated_text
        );

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

        let response = self
            .client
            .post_with_retry("Gemini 3.1 Flash TTS", || {
                let cli = http_client.clone();
                let url = target_endpoint.clone();
                let bytes = body_bytes.clone();
                async move {
                    cli.post(&url)
                        .header("Content-Type", "application/json")
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

        let mut file = File::create(output_path).map_err(|e| {
            DomainError::Internal(format!("Failed to create segment audio file: {}", e))
        })?;

        file.write_all(&output_bytes).map_err(|e| {
            DomainError::Internal(format!("Failed to write segment audio bytes: {}", e))
        })?;
        drop(file);

        // Probe the segment duration
        let p = output_path.to_path_buf();
        let metadata = tokio::task::spawn_blocking(move || FfprobeInspector::probe(&p))
            .await
            .map_err(|e| DomainError::Internal(format!("Task error: {}", e)))??;

        Ok(SynthesizedSegment {
            segment_id: segment.segment_id.clone(),
            speaker_id: segment.speaker_id.clone(),
            path: output_path.to_path_buf(),
            duration_ms: metadata.duration_ms,
        })
    }
}
