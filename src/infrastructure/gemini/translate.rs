use super::client::GeminiClient;
use crate::application::ports::TextTranslator;
use crate::domain::{
    DomainError, LanguageId, Transcript, TranscriptSegment, TranslatedDocument, TranslationSegment,
    TranslationTone,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize)]
struct GeminiGenerateResponse {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Debug, Deserialize)]
struct CandidateContent {
    parts: Option<Vec<Part>>,
}

#[derive(Debug, Deserialize)]
struct Part {
    text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RawTranslatedItem {
    segment_id: String,
    translated_text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TranslationResponsePayload {
    translations: Vec<RawTranslatedItem>,
}

pub struct GeminiTranslator {
    client: GeminiClient,
    model_name: String,
}

impl GeminiTranslator {
    pub fn new(client: GeminiClient, model_name: impl Into<String>) -> Self {
        Self {
            client,
            model_name: model_name.into(),
        }
    }

    async fn translate_chunk(
        &self,
        segments: &[TranscriptSegment],
        target_lang: &LanguageId,
        tone: TranslationTone,
    ) -> Result<Vec<RawTranslatedItem>, DomainError> {
        if segments.is_empty() {
            return Ok(Vec::new());
        }

        let endpoint = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.client.base_url(),
            self.model_name,
            self.client.api_key()
        );

        // Prepare segments JSON to provide to model
        let segments_json: Vec<serde_json::Value> = segments
            .iter()
            .map(|s| {
                json!({
                    "segment_id": s.id,
                    "speaker": s.speaker_id,
                    "text": s.text
                })
            })
            .collect();

        let prompt = format!(
            r#"You are a professional audio dubbing translator.
Translate the following spoken transcript from source language into target language: '{target_language}'.

Style and Tone Guidance:
{tone_instruction}

Requirements:
- Preserve original meaning and conversational spoken flow.
- Make the phrasing concise and natural for speech dubbing.
- Preserve names, numbers, dates, and technical terminology accurately.
- Maintain the exact same segment_id for each item.
- Do not add explanations or meta text.
- Output strictly in valid JSON matching the schema:
  {{ "translations": [ {{ "segment_id": "seg_0001", "translated_text": "..." }} ] }}

Transcript segments:
{segments_data}
"#,
            target_language = target_lang.as_str(),
            tone_instruction = tone.prompt_directive(),
            segments_data = serde_json::to_string_pretty(&segments_json).unwrap_or_default()
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
                "responseMimeType": "application/json",
                "temperature": 0.3
            }
        });

        let body_bytes = serde_json::to_vec(&request_body).map_err(|e| {
            DomainError::Internal(format!("Failed to serialize translate request: {}", e))
        })?;

        let http_client = self.client.http().clone();
        let target_endpoint = endpoint.clone();

        let response = self
            .client
            .post_with_retry("Gemini 3.1 Flash-Lite Translation", || {
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

        let gen_response: GeminiGenerateResponse = response.json().await.map_err(|e| {
            DomainError::PermanentApiError(format!(
                "Failed to parse translation API response: {}",
                e
            ))
        })?;

        let text_content = gen_response
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
            .and_then(|p| p.text)
            .ok_or_else(|| {
                DomainError::PermanentApiError("Empty translation content returned".to_string())
            })?;

        // Parse structured JSON output
        let parsed_translations: TranslationResponsePayload = serde_json::from_str(&text_content)
            .or_else(|_| {
                // Fallback: try parsing as a direct array if the wrapper was omitted
                serde_json::from_str::<Vec<RawTranslatedItem>>(&text_content).map(|items| {
                    TranslationResponsePayload {
                        translations: items,
                    }
                })
            })
            .map_err(|e| {
                DomainError::PermanentApiError(format!(
                    "Failed to parse JSON translation structure: {}. Response: {}",
                    e, text_content
                ))
            })?;

        Ok(parsed_translations.translations)
    }
}

#[async_trait]
impl TextTranslator for GeminiTranslator {
    async fn translate(
        &self,
        transcript: &Transcript,
        target_lang: &LanguageId,
        tone: TranslationTone,
    ) -> Result<TranslatedDocument, DomainError> {
        if transcript.segments.is_empty() {
            return Err(DomainError::PermanentApiError(
                "Cannot translate empty transcript".to_string(),
            ));
        }

        // Chunking rule: process in chunks of 20 segments to prevent token exhaustion and missing items
        const CHUNK_SIZE: usize = 20;
        let mut raw_translations = Vec::new();

        for chunk in transcript.segments.chunks(CHUNK_SIZE) {
            let chunk_items = self.translate_chunk(chunk, target_lang, tone).await?;
            raw_translations.extend(chunk_items);
        }

        let mut translated_segments = Vec::new();

        for source_seg in &transcript.segments {
            let matched = raw_translations
                .iter()
                .find(|t| t.segment_id == source_seg.id);

            let translated_text = match matched {
                Some(t) if !t.translated_text.trim().is_empty() => {
                    t.translated_text.trim().to_string()
                }
                _ => source_seg.text.clone(), // Fallback to source text if missing
            };

            translated_segments.push(TranslationSegment {
                segment_id: source_seg.id.clone(),
                speaker_id: source_seg.speaker_id.clone(),
                source_start_ms: source_seg.start_ms,
                source_end_ms: source_seg.end_ms,
                source_text: source_seg.text.clone(),
                translated_text,
            });
        }

        Ok(TranslatedDocument::new(
            transcript.language.clone(),
            target_lang.clone(),
            translated_segments,
        ))
    }
}
