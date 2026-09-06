use super::client::GeminiClient;
use super::files::GeminiFilesApi;
use crate::application::ports::SpeechTranscriber;
use crate::domain::{
    AudioDocument, DomainError, LanguageId, Transcript, TranscriptSegment, WordTimestamp,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct InteractionWord {
    word: Option<String>,
    #[serde(rename = "startTime")]
    start_time: Option<String>,
    #[serde(rename = "endTime")]
    end_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InteractionSegment {
    id: Option<String>,
    text: Option<String>,
    speaker: Option<String>,
    #[serde(rename = "startTime")]
    start_time: Option<String>,
    #[serde(rename = "endTime")]
    end_time: Option<String>,
    words: Option<Vec<InteractionWord>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct InteractionResult {
    text: Option<String>,
    language: Option<String>,
    segments: Option<Vec<InteractionSegment>>,
}

#[derive(Debug, Deserialize)]
struct InteractionResponse {
    result: Option<InteractionResult>,
    text: Option<String>,
}

pub struct GeminiTranscriber {
    client: GeminiClient,
    files_api: GeminiFilesApi,
    model_name: String,
}

impl GeminiTranscriber {
    pub fn new(client: GeminiClient, model_name: impl Into<String>) -> Self {
        let files_api = GeminiFilesApi::new(client.clone());
        Self {
            client,
            files_api,
            model_name: model_name.into(),
        }
    }

    fn parse_time_to_ms(time_str: Option<&str>) -> u64 {
        let s = match time_str {
            Some(val) => val.trim().trim_end_matches('s'),
            None => return 0,
        };

        s.parse::<f64>()
            .map(|secs| (secs * 1000.0).round() as u64)
            .unwrap_or(0)
    }
}

#[async_trait]
impl SpeechTranscriber for GeminiTranscriber {
    async fn transcribe(
        &self,
        audio: &AudioDocument,
        source_hint: &LanguageId,
    ) -> Result<Transcript, DomainError> {
        // 1. Upload audio file via Gemini Files API
        let file_info = self
            .files_api
            .upload_file(&audio.path, &audio.mime_type)
            .await?;

        // 2. Prepare request for Interactions API
        let endpoint = format!(
            "{}/v1beta/interactions?key={}",
            self.client.base_url(),
            self.client.api_key()
        );

        let mut request_body = json!({
            "model": self.model_name,
            "input": [
                {
                    "type": "audio",
                    "uri": file_info.uri,
                    "mime_type": audio.mime_type
                }
            ],
            "parameters": {
                "enable_diarization": true,
                "enable_word_timestamps": true
            }
        });

        if !source_hint.is_auto() {
            request_body["parameters"]["language_code"] = json!(source_hint.as_str());
        }

        let body_bytes = serde_json::to_vec(&request_body).map_err(|e| {
            DomainError::Internal(format!("Failed to serialize transcribe request: {}", e))
        })?;

        let http_client = self.client.http().clone();
        let target_endpoint = endpoint.clone();

        let response = self
            .client
            .post_with_retry("Gemini 3.5 Transcribe", || {
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

        let response_data: InteractionResponse = response.json().await.map_err(|e| {
            DomainError::PermanentApiError(format!("Failed to parse transcribe response: {}", e))
        })?;

        // 3. Cleanup uploaded file on Google side asynchronously
        let files_api = self.files_api.clone();
        let file_name = file_info.name.clone();
        tokio::spawn(async move {
            let _ = files_api.delete_file(&file_name).await;
        });

        // 4. Map response to Domain Transcript
        let detected_lang_code = response_data
            .result
            .as_ref()
            .and_then(|r| r.language.clone())
            .unwrap_or_else(|| {
                if !source_hint.is_auto() {
                    source_hint.as_str().to_string()
                } else {
                    "id".to_string()
                }
            });

        let detected_language = LanguageId::new(detected_lang_code);

        let mut segments = Vec::new();

        if let Some(res_segments) = response_data.result.and_then(|r| r.segments) {
            for (idx, seg) in res_segments.into_iter().enumerate() {
                let seg_id = seg.id.unwrap_or_else(|| format!("seg_{:04}", idx + 1));
                let text = seg.text.unwrap_or_default().trim().to_string();
                if text.is_empty() {
                    continue;
                }

                let start_ms = Self::parse_time_to_ms(seg.start_time.as_deref());
                let end_ms = Self::parse_time_to_ms(seg.end_time.as_deref());
                let safe_end_ms = if end_ms > start_ms {
                    end_ms
                } else {
                    start_ms + 2000
                };

                let words = seg
                    .words
                    .unwrap_or_default()
                    .into_iter()
                    .map(|w| WordTimestamp {
                        word: w.word.unwrap_or_default(),
                        start_ms: Self::parse_time_to_ms(w.start_time.as_deref()),
                        end_ms: Self::parse_time_to_ms(w.end_time.as_deref()),
                    })
                    .collect();

                segments.push(TranscriptSegment {
                    id: seg_id,
                    speaker_id: seg.speaker,
                    start_ms,
                    end_ms: safe_end_ms,
                    text,
                    words,
                });
            }
        }

        // Fallback: If no segments were structured, create single segment from full text
        if segments.is_empty() {
            let fallback_text = response_data.text.unwrap_or_default().trim().to_string();
            if !fallback_text.is_empty() {
                segments.push(TranscriptSegment {
                    id: "seg_0001".to_string(),
                    speaker_id: Some("Speaker 1".to_string()),
                    start_ms: 0,
                    end_ms: audio.metadata.duration_ms,
                    text: fallback_text,
                    words: Vec::new(),
                });
            }
        }

        if segments.is_empty() {
            return Err(DomainError::PermanentApiError(
                "Gemini Transcribe returned empty transcript".to_string(),
            ));
        }

        Ok(Transcript::new(detected_language, segments))
    }
}
