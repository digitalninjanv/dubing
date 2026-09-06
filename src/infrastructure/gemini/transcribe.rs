use super::client::GeminiClient;
use super::files::GeminiFilesApi;
use crate::application::ports::SpeechTranscriber;
use crate::domain::{
    AudioDocument, DomainError, LanguageId, Transcript, TranscriptSegment, WordTimestamp,
};
use async_trait::async_trait;
use base64::Engine;
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

#[derive(Debug, Deserialize)]
struct FallbackSegment {
    id: Option<String>,
    speaker: Option<String>,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FallbackTranscriptJson {
    language: Option<String>,
    segments: Option<Vec<FallbackSegment>>,
}

#[derive(Debug, Deserialize)]
struct CandidatePart {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CandidateContent {
    parts: Option<Vec<CandidatePart>>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    candidates: Option<Vec<Candidate>>,
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

    pub async fn fallback_transcribe(
        &self,
        audio: &AudioDocument,
        file_info: &super::files::GeminiFileInfo,
        source_hint: &LanguageId,
    ) -> Result<Transcript, DomainError> {
        let models = ["gemini-2.5-flash", "gemini-2.0-flash", "gemini-1.5-flash"];
        let mut last_err = None;

        for model in models {
            let endpoint = format!(
                "{}/v1beta/models/{}:generateContent?key={}",
                self.client.base_url(),
                model,
                self.client.api_key()
            );

            let lang_instruction = if source_hint.is_auto() {
                "Detect the spoken language automatically and specify its ISO 639-1 code in the 'language' field.".to_string()
            } else {
                format!(
                    "The audio is spoken in language code '{}'.",
                    source_hint.as_str()
                )
            };

            let prompt = format!(
                r#"You are a professional audio transcription and speaker diarization system.
{}
Transcribe all spoken sentences from the audio accurately.
Segment each utterance/sentence naturally with start and end timestamps in milliseconds (audio total duration is {} ms).
Identify distinct speakers (e.g. 'Speaker 1', 'Speaker 2').

Respond with ONLY a valid JSON object matching this schema:
{{
  "language": "id",
  "segments": [
    {{
      "id": "seg_0001",
      "speaker": "Speaker 1",
      "start_ms": 0,
      "end_ms": 2500,
      "text": "transcribed speech here"
    }}
  ]
}}"#,
                lang_instruction, audio.metadata.duration_ms
            );

            let request_body = json!({
                "contents": [
                    {
                        "role": "user",
                        "parts": [
                            {
                                "file_data": {
                                    "file_uri": file_info.uri,
                                    "mime_type": audio.mime_type
                                }
                            },
                            {
                                "text": prompt
                            }
                        ]
                    }
                ],
                "generationConfig": {
                    "responseMimeType": "application/json"
                }
            });

            let body_bytes = serde_json::to_vec(&request_body).map_err(|e| {
                DomainError::Internal(format!(
                    "Failed to serialize fallback transcribe request: {}",
                    e
                ))
            })?;

            let http_client = self.client.http().clone();
            let target_endpoint = endpoint.clone();
            let api_key = self.client.api_key().to_string();

            let resp_res = self
                .client
                .post_with_retry(&format!("Gemini Fallback Transcribe ({})", model), || {
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
                .await;

            match resp_res {
                Ok(response) => {
                    if let Ok(gen_resp) = response.json::<GenerateContentResponse>().await {
                        if let Some(candidate) =
                            gen_resp.candidates.and_then(|c| c.into_iter().next())
                        {
                            if let Some(part) = candidate
                                .content
                                .and_then(|c| c.parts)
                                .and_then(|p| p.into_iter().next())
                            {
                                if let Some(raw_json) = part.text {
                                    if let Ok(parsed) =
                                        serde_json::from_str::<FallbackTranscriptJson>(&raw_json)
                                    {
                                        let detected_lang =
                                            LanguageId::new(parsed.language.unwrap_or_else(|| {
                                                if !source_hint.is_auto() {
                                                    source_hint.as_str().to_string()
                                                } else {
                                                    "id".to_string()
                                                }
                                            }));

                                        let mut segments = Vec::new();
                                        if let Some(segs) = parsed.segments {
                                            for (idx, s) in segs.into_iter().enumerate() {
                                                let text =
                                                    s.text.unwrap_or_default().trim().to_string();
                                                if text.is_empty() {
                                                    continue;
                                                }
                                                let seg_id = s.id.unwrap_or_else(|| {
                                                    format!("seg_{:04}", idx + 1)
                                                });
                                                let start_ms = s.start_ms.unwrap_or(0);
                                                let end_ms = s.end_ms.unwrap_or(start_ms + 2000);
                                                let safe_end_ms = if end_ms > start_ms {
                                                    end_ms
                                                } else {
                                                    start_ms + 2000
                                                };
                                                segments.push(TranscriptSegment {
                                                    id: seg_id,
                                                    speaker_id: s
                                                        .speaker
                                                        .or_else(|| Some("Speaker 1".to_string())),
                                                    start_ms,
                                                    end_ms: safe_end_ms,
                                                    text,
                                                    words: Vec::new(),
                                                });
                                            }
                                        }

                                        if !segments.is_empty() {
                                            return Ok(Transcript::new(detected_lang, segments));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            DomainError::PermanentApiError("All transcription fallback attempts failed".to_string())
        }))
    }

    /// High-Speed Direct Ingestion Fast-Path:
    /// For files < 20MB, transcribes audio via inline base64 data directly with Gemini 2.5 Flash.
    /// Bypasses Files API upload, async processing polling, and deletion delays entirely.
    pub async fn transcribe_inline(
        &self,
        audio: &AudioDocument,
        source_hint: &LanguageId,
    ) -> Result<Transcript, DomainError> {
        let file_bytes = tokio::fs::read(&audio.path).await.map_err(|e| {
            DomainError::InvalidAudio(format!(
                "Failed to read audio file for inline transcribe: {}",
                e
            ))
        })?;

        let b64_audio = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
        let models = ["gemini-2.5-flash", "gemini-2.0-flash", "gemini-1.5-flash"];
        let mut last_err = None;

        let lang_instruction = if source_hint.is_auto() {
            "Detect the spoken language automatically and specify its ISO 639-1 code in the 'language' field.".to_string()
        } else {
            format!(
                "The audio is spoken in language code '{}'.",
                source_hint.as_str()
            )
        };

        let prompt = format!(
            r#"You are a professional audio transcription and speaker diarization system.
{}
Transcribe all spoken sentences from the audio accurately.
Segment each utterance/sentence naturally with start and end timestamps in milliseconds (audio total duration is {} ms).
Identify distinct speakers (e.g. 'Speaker 1', 'Speaker 2').

Respond with ONLY a valid JSON object matching this schema:
{{
  "language": "id",
  "segments": [
    {{
      "id": "seg_0001",
      "speaker": "Speaker 1",
      "start_ms": 0,
      "end_ms": 2500,
      "text": "transcribed speech here"
    }}
  ]
}}"#,
            lang_instruction, audio.metadata.duration_ms
        );

        let request_body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {
                            "inline_data": {
                                "mime_type": audio.mime_type,
                                "data": b64_audio
                            }
                        },
                        {
                            "text": prompt
                        }
                    ]
                }
            ],
            "generationConfig": {
                "responseMimeType": "application/json"
            }
        });

        let body_bytes = serde_json::to_vec(&request_body).map_err(|e| {
            DomainError::Internal(format!(
                "Failed to serialize inline transcribe request: {}",
                e
            ))
        })?;

        for model in models {
            let endpoint = format!(
                "{}/v1beta/models/{}:generateContent?key={}",
                self.client.base_url(),
                model,
                self.client.api_key()
            );

            let http_client = self.client.http().clone();
            let target_endpoint = endpoint.clone();
            let bytes = body_bytes.clone();
            let api_key = self.client.api_key().to_string();

            let resp_res = self
                .client
                .post_with_retry(&format!("Gemini Inline Transcribe ({})", model), || {
                    let cli = http_client.clone();
                    let url = target_endpoint.clone();
                    let b = bytes.clone();
                    let key = api_key.clone();
                    async move {
                        cli.post(&url)
                            .header("Content-Type", "application/json")
                            .header("x-goog-api-key", key)
                            .body(b)
                            .send()
                            .await
                    }
                })
                .await;

            match resp_res {
                Ok(response) => {
                    if let Ok(gen_resp) = response.json::<GenerateContentResponse>().await {
                        if let Some(candidate) =
                            gen_resp.candidates.and_then(|c| c.into_iter().next())
                        {
                            if let Some(part) = candidate
                                .content
                                .and_then(|c| c.parts)
                                .and_then(|p| p.into_iter().next())
                            {
                                if let Some(raw_json) = part.text {
                                    if let Ok(parsed) =
                                        serde_json::from_str::<FallbackTranscriptJson>(&raw_json)
                                    {
                                        let detected_lang =
                                            LanguageId::new(parsed.language.unwrap_or_else(|| {
                                                if !source_hint.is_auto() {
                                                    source_hint.as_str().to_string()
                                                } else {
                                                    "id".to_string()
                                                }
                                            }));

                                        let mut segments = Vec::new();
                                        if let Some(segs) = parsed.segments {
                                            for (idx, s) in segs.into_iter().enumerate() {
                                                let text =
                                                    s.text.unwrap_or_default().trim().to_string();
                                                if text.is_empty() {
                                                    continue;
                                                }
                                                let seg_id = s.id.unwrap_or_else(|| {
                                                    format!("seg_{:04}", idx + 1)
                                                });
                                                let start_ms = s.start_ms.unwrap_or(0);
                                                let end_ms = s.end_ms.unwrap_or(start_ms + 2000);
                                                let safe_end_ms = if end_ms > start_ms {
                                                    end_ms
                                                } else {
                                                    start_ms + 2000
                                                };
                                                segments.push(TranscriptSegment {
                                                    id: seg_id,
                                                    speaker_id: s
                                                        .speaker
                                                        .or_else(|| Some("Speaker 1".to_string())),
                                                    start_ms,
                                                    end_ms: safe_end_ms,
                                                    text,
                                                    words: Vec::new(),
                                                });
                                            }
                                        }

                                        if !segments.is_empty() {
                                            return Ok(Transcript::new(detected_lang, segments));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            DomainError::PermanentApiError("Inline transcription failed".to_string())
        }))
    }
}

#[async_trait]
impl SpeechTranscriber for GeminiTranscriber {
    async fn transcribe(
        &self,
        audio: &AudioDocument,
        source_hint: &LanguageId,
    ) -> Result<Transcript, DomainError> {
        // Fast Path Optimization: If file size is under 20MB, transcribe directly via high-speed inline data
        let file_size = std::fs::metadata(&audio.path).map(|m| m.len()).unwrap_or(0);
        const MAX_INLINE_BYTES: u64 = 20 * 1024 * 1024; // 20 MB

        if file_size > 0 && file_size < MAX_INLINE_BYTES {
            tracing::info!(
                "Audio file ({} bytes) is under 20MB: using high-speed inline transcription fast-path",
                file_size
            );
            match self.transcribe_inline(audio, source_hint).await {
                Ok(transcript) => return Ok(transcript),
                Err(err) => {
                    tracing::warn!(
                        "Direct inline transcription attempt failed ({}). Falling back to Files API...",
                        err
                    );
                }
            }
        }

        // Fallback or Large Files (>= 20MB): Upload audio file via Gemini Files API
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

        let mut transcription_config = json!({
            "mode": {
                "type": "verbatim",
                "diarization_mode": "speaker"
            }
        });

        if !source_hint.is_auto() {
            transcription_config["language_code"] = json!(source_hint.as_str());
        }

        let request_body = json!({
            "model": self.model_name,
            "input": [
                {
                    "type": "audio",
                    "uri": file_info.uri
                }
            ],
            "generation_config": {
                "transcription_config": transcription_config
            }
        });

        let body_bytes = serde_json::to_vec(&request_body).map_err(|e| {
            DomainError::Internal(format!("Failed to serialize transcribe request: {}", e))
        })?;

        let http_client = self.client.http().clone();
        let target_endpoint = endpoint.clone();
        let api_key = self.client.api_key().to_string();

        let response_res = self
            .client
            .post_with_retry("Gemini 3.5 Transcribe", || {
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
            .await;

        let transcript_result = match response_res {
            Ok(response) => match response.json::<InteractionResponse>().await {
                Ok(response_data) => {
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

                    if segments.is_empty() {
                        let fallback_text =
                            response_data.text.unwrap_or_default().trim().to_string();
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
                        tracing::warn!("Interactions API returned empty segments. Attempting fallback transcription...");
                        self.fallback_transcribe(audio, &file_info, source_hint)
                            .await
                    } else {
                        Ok(Transcript::new(detected_language, segments))
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to parse Interactions API response: {}. Attempting fallback transcription...",
                        err
                    );
                    self.fallback_transcribe(audio, &file_info, source_hint)
                        .await
                }
            },
            Err(err) => {
                tracing::warn!(
                    "Interactions API transcription returned error ({}). Falling back to multimodal transcription as per safety policy...",
                    err
                );
                self.fallback_transcribe(audio, &file_info, source_hint)
                    .await
            }
        };

        // 3. Cleanup uploaded file on Google side asynchronously
        let files_api = self.files_api.clone();
        let file_name = file_info.name.clone();
        tokio::spawn(async move {
            let _ = files_api.delete_file(&file_name).await;
        });

        transcript_result
    }
}
