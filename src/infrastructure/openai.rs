use crate::application::ports::{SpeechSynthesizer, SpeechTranscriber, TextTranslator};
use crate::domain::{
    DomainError, LanguageId, SynthesizedSegment, Transcript, TranscriptSegment, TranslatedDocument,
    TranslationSegment, TranslationTone, VoiceProfile, WordTimestamp,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

const DEFAULT_BASE_URL: &str = "https://api.openai.com";

#[derive(Clone)]
pub struct OpenAiClient {
    http: Client,
    api_key: Arc<str>,
    base_url: Arc<str>,
}

impl OpenAiClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            api_key: Arc::from(api_key.into()),
            base_url: Arc::from(DEFAULT_BASE_URL),
        }
    }

    fn auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .bearer_auth(self.api_key.as_ref())
            .header("Accept", "application/json")
    }

    async fn ensure_success(
        response: reqwest::Response,
        operation: &str,
    ) -> Result<reqwest::Response, DomainError> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().await.unwrap_or_default();
        let body = if body.len() > 800 {
            format!("{}…", &body[..800])
        } else {
            body
        };
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(DomainError::AuthenticationFailed);
        }
        if status.as_u16() == 429 || status.is_server_error() {
            return Err(DomainError::TransientError(format!(
                "{} returned HTTP {}: {}",
                operation, status, body
            )));
        }
        Err(DomainError::PermanentApiError(format!(
            "{} returned HTTP {}: {}",
            operation, status, body
        )))
    }
}

pub struct OpenAiTranscriber {
    client: OpenAiClient,
    model_name: String,
}

impl OpenAiTranscriber {
    pub fn new(client: OpenAiClient, model_name: impl Into<String>) -> Self {
        Self {
            client,
            model_name: model_name.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: Option<String>,
}

#[async_trait]
impl SpeechTranscriber for OpenAiTranscriber {
    async fn transcribe(
        &self,
        audio: &crate::domain::AudioDocument,
        source_hint: &LanguageId,
    ) -> Result<Transcript, DomainError> {
        let bytes = tokio::fs::read(&audio.path).await.map_err(|e| {
            DomainError::InvalidAudio(format!("Failed to read audio for OpenAI: {}", e))
        })?;

        let mut form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes)
                    .file_name(
                        audio
                            .path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("audio.wav")
                            .to_string(),
                    )
                    .mime_str(&audio.mime_type)
                    .map_err(|e| {
                        DomainError::Internal(format!("Invalid audio MIME type: {}", e))
                    })?,
            )
            .text("model", self.model_name.clone())
            .text("response_format", "json".to_string());

        if !source_hint.is_auto() {
            form = form.text("language", source_hint.as_str().to_string());
        }

        let response = self
            .client
            .auth(
                self.client
                    .http
                    .post(format!("{}/v1/audio/transcriptions", self.client.base_url)),
            )
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                DomainError::TransientError(format!("OpenAI transcription network error: {}", e))
            })?;

        let response = OpenAiClient::ensure_success(response, "OpenAI transcription").await?;
        let parsed: TranscriptionResponse = response.json().await.map_err(|e| {
            DomainError::PermanentApiError(format!("Invalid OpenAI transcription response: {}", e))
        })?;

        let text = parsed.text.unwrap_or_default().trim().to_string();
        if text.is_empty() {
            return Err(DomainError::QualityGate(
                "OpenAI transcription returned empty text".to_string(),
            ));
        }

        let language = if source_hint.is_auto() {
            LanguageId::new("auto")
        } else {
            source_hint.clone()
        };

        Ok(Transcript::new(
            language,
            vec![TranscriptSegment {
                id: "seg_0001".to_string(),
                speaker_id: Some("Speaker 1".to_string()),
                start_ms: 0,
                end_ms: audio.metadata.duration_ms.max(1),
                text,
                words: Vec::<WordTimestamp>::new(),
            }],
        ))
    }
}

pub struct OpenAiTranslator {
    client: OpenAiClient,
    model_name: String,
}

impl OpenAiTranslator {
    pub fn new(client: OpenAiClient, model_name: impl Into<String>) -> Self {
        Self {
            client,
            model_name: model_name.into(),
        }
    }
}

fn extract_response_text(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.get("output_text").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }

    fn walk(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                    return Some(text.to_string());
                }
                for child in map.values() {
                    if let Some(found) = walk(child) {
                        return Some(found);
                    }
                }
                None
            }
            serde_json::Value::Array(items) => items.iter().find_map(walk),
            _ => None,
        }
    }

    walk(value)
}

#[async_trait]
impl TextTranslator for OpenAiTranslator {
    fn cache_identity(&self) -> String {
        format!("openai:{}", self.model_name)
    }

    async fn translate(
        &self,
        transcript: &Transcript,
        target_lang: &LanguageId,
        tone: TranslationTone,
    ) -> Result<TranslatedDocument, DomainError> {
        let segments: Vec<_> = transcript
            .segments
            .iter()
            .map(|s| {
                json!({
                    "segment_id": s.id,
                    "speaker_id": s.speaker_id,
                    "start_ms": s.start_ms,
                    "end_ms": s.end_ms,
                    "text": s.text
                })
            })
            .collect();

        let prompt = format!(
            "Translate this dubbing transcript into {}. Tone: {}. Preserve segment_id,              speaker_id and timing fields exactly. Return ONLY JSON:              {{"translations":[{{"segment_id":"...","translated_text":"..."}}]}}              Keep translations concise enough to fit the original segment duration.\n{}",
            target_lang.as_str(),
            tone.as_str(),
            serde_json::to_string(&segments).unwrap_or_default()
        );

        let response = self
            .client
            .auth(
                self.client
                    .http
                    .post(format!("{}/v1/responses", self.client.base_url)),
            )
            .json(&json!({
                "model": self.model_name,
                "input": prompt,
            }))
            .send()
            .await
            .map_err(|e| {
                DomainError::TransientError(format!("OpenAI translation network error: {}", e))
            })?;

        let response = OpenAiClient::ensure_success(response, "OpenAI translation").await?;
        let value: serde_json::Value = response.json().await.map_err(|e| {
            DomainError::PermanentApiError(format!("Invalid OpenAI translation response: {}", e))
        })?;
        let raw = extract_response_text(&value).ok_or_else(|| {
            DomainError::PermanentApiError(
                "OpenAI translation response contained no text".to_string(),
            )
        })?;

        #[derive(Deserialize)]
        struct Payload {
            translations: Vec<Item>,
        }
        #[derive(Deserialize)]
        struct Item {
            segment_id: String,
            translated_text: String,
        }

        let payload: Payload = serde_json::from_str(&raw).map_err(|e| {
            DomainError::PermanentApiError(format!("OpenAI translation JSON was invalid: {}", e))
        })?;

        let mut by_id = std::collections::HashMap::new();
        for item in payload.translations {
            by_id.insert(item.segment_id, item.translated_text);
        }

        let translated = transcript
            .segments
            .iter()
            .map(|s| TranslationSegment {
                segment_id: s.id.clone(),
                speaker_id: s.speaker_id.clone(),
                source_start_ms: s.start_ms,
                source_end_ms: s.end_ms,
                source_text: s.text.clone(),
                translated_text: by_id.remove(&s.id).unwrap_or_else(|| s.text.clone()),
            })
            .collect();

        Ok(TranslatedDocument::new(
            transcript.language.clone(),
            target_lang.clone(),
            translated,
        ))
    }
}

pub struct OpenAiSynthesizer {
    client: OpenAiClient,
    model_name: String,
}

impl OpenAiSynthesizer {
    pub fn new(client: OpenAiClient, model_name: impl Into<String>) -> Self {
        Self {
            client,
            model_name: model_name.into(),
        }
    }
}

fn map_voice(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "alloy" => "alloy",
        "ash" => "ash",
        "ballad" => "ballad",
        "coral" => "coral",
        "echo" => "echo",
        "fable" => "fable",
        "onyx" => "onyx",
        "nova" => "nova",
        "sage" => "sage",
        "shimmer" => "shimmer",
        "verse" => "verse",
        "marin" => "marin",
        "cedar" => "cedar",
        _ => "alloy",
    }
}

#[async_trait]
impl SpeechSynthesizer for OpenAiSynthesizer {
    fn cache_identity(&self) -> String {
        format!("openai:{}", self.model_name)
    }

    async fn synthesize_segment(
        &self,
        segment: &TranslationSegment,
        voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        let mut request = json!({
            "model": self.model_name,
            "input": segment.translated_text,
            "voice": map_voice(&voice.voice_name),
            "response_format": "wav",
            "speed": voice.speed.clamp(0.25, 4.0),
        });

        if let Some(style) = &voice.style {
            request["instructions"] = json!(style);
        }

        let response = self
            .client
            .auth(
                self.client
                    .http
                    .post(format!("{}/v1/audio/speech", self.client.base_url)),
            )
            .json(&request)
            .send()
            .await
            .map_err(|e| DomainError::TransientError(format!("OpenAI TTS network error: {}", e)))?;

        let response = OpenAiClient::ensure_success(response, "OpenAI TTS").await?;
        let bytes = response.bytes().await.map_err(|e| {
            DomainError::PermanentApiError(format!("Failed to read OpenAI TTS audio: {}", e))
        })?;

        if bytes.is_empty() {
            return Err(DomainError::QualityGate(
                "OpenAI TTS returned an empty audio payload".to_string(),
            ));
        }

        tokio::fs::write(output_path, &bytes).await.map_err(|e| {
            DomainError::ExportError(format!("Failed to save OpenAI TTS audio: {}", e))
        })?;

        Ok(SynthesizedSegment {
            segment_id: segment.segment_id.clone(),
            speaker_id: segment.speaker_id.clone(),
            path: output_path.to_path_buf(),
            duration_ms: segment.target_duration_ms().max(1),
        })
    }
}
