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

        // Prepare segments JSON to provide to model with strict character budget
        let segments_json: Vec<serde_json::Value> = segments
            .iter()
            .map(|s| {
                let duration_ms = s.duration_ms();
                let duration_secs = (duration_ms as f64) / 1000.0;
                let is_cjk = target_lang.as_str() == "ja"
                    || target_lang.as_str() == "zh"
                    || target_lang.as_str() == "ko";
                // Estimate character budget: CJK ~6 chars/sec, Latin/other ~15 chars/sec
                let max_character_limit = if is_cjk {
                    ((duration_secs * 6.0).round() as usize).max(4)
                } else {
                    ((duration_secs * 15.0).round() as usize).max(10)
                };

                json!({
                    "segment_id": s.id,
                    "speaker": s.speaker_id,
                    "text": s.text,
                    "duration_ms": duration_ms,
                    "max_character_limit": max_character_limit,
                })
            })
            .collect();

        let prompt = format!(
            r#"You are a professional audio dubbing translator and script adapter.
Translate the following spoken transcript from source language into target language: '{target_language}'.

Style and Tone Guidance:
{tone_instruction}

Isochronous Dubbing Constraints & Timing Rules:
- STRICT TIMING BUDGET: Each segment has a strict time budget ('duration_ms') and an explicit 'max_character_limit'.
- Your translation MUST be speakable within 'duration_ms'. Do NOT exceed 'max_character_limit'.
- Avoid wordy, overly formal, or verbose phrasing. Use concise, colloquial, and punchy wording that preserves the core message and emotional intent.
- If the literal translation would be too long, condense sentences or use shorter synonyms so the voice actor can finish speaking naturally within the duration.
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
        let mut candidates = vec![self.model_name.as_str()];
        let fallbacks = ["gemini-3.5-flash-lite", "gemini-3.5-flash"];
        for fb in fallbacks {
            if !candidates.contains(&fb) {
                candidates.push(fb);
            }
        }

        let mut last_err = None;
        for (i, &model) in candidates.iter().enumerate() {
            if i > 0 {
                tracing::info!(
                    "Fallback cascade: attempting translation with model '{}'",
                    model
                );
            }

            let endpoint = format!(
                "{}/v1beta/models/{}:generateContent?key={}",
                self.client.base_url(),
                model,
                self.client.api_key()
            );

            let op_name = format!("Gemini Translation ({})", model);
            let target_endpoint = endpoint.clone();
            let cli = http_client.clone();
            let bytes = body_bytes.clone();
            let api_key = self.client.api_key().to_string();

            let response = match self
                .client
                .post_with_retry(&op_name, move || {
                    let c = cli.clone();
                    let u = target_endpoint.clone();
                    let b = bytes.clone();
                    let k = api_key.clone();
                    async move {
                        c.post(&u)
                            .header("Content-Type", "application/json")
                            .header("x-goog-api-key", k)
                            .body(b)
                            .send()
                            .await
                    }
                })
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    if e.is_retryable() {
                        tracing::warn!(
                            "Translation model '{}' hit retryable rate-limit error ({}). Cascading to next model...",
                            model,
                            e
                        );
                        last_err = Some(e);
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            };

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
            let parsed_translations: TranslationResponsePayload =
                serde_json::from_str(&text_content)
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

            return Ok(parsed_translations.translations);
        }

        Err(last_err.unwrap_or_else(|| {
            DomainError::TransientError("All candidate translation models exhausted".to_string())
        }))
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

        let chunks: Vec<&[TranscriptSegment]> = transcript.segments.chunks(CHUNK_SIZE).collect();
        if chunks.len() <= 1 {
            if let Some(first_chunk) = chunks.first() {
                let chunk_items = self.translate_chunk(first_chunk, target_lang, tone).await?;
                raw_translations.extend(chunk_items);
            }
        } else {
            // Bounded concurrency (3) to avoid burst 429s on large transcripts;
            // preserves input order via indexed results (chunks are cloned to
            // satisfy 'static bounds of the concurrent stream).
            use futures::stream::{self, StreamExt};
            let indexed: Vec<(usize, Vec<TranscriptSegment>)> = chunks
                .iter()
                .enumerate()
                .map(|(i, c)| (i, c.to_vec()))
                .collect();
            let mut stream = stream::iter(indexed)
                .map(|(i, chunk)| {
                    let target = target_lang.clone();
                    async move {
                        let items = self.translate_chunk(&chunk, &target, tone).await?;
                        Ok::<_, DomainError>((i, items))
                    }
                })
                .buffer_unordered(3);
            let mut ordered: Vec<(usize, Vec<RawTranslatedItem>)> =
                Vec::with_capacity(chunks.len());
            while let Some(res) = stream.next().await {
                ordered.push(res?);
            }
            ordered.sort_by_key(|(i, _)| *i);
            for (_, chunk_items) in ordered {
                raw_translations.extend(chunk_items);
            }
        }

        // F5: O(1) lookup via HashMap instead of O(n²) find loop.
        let map: std::collections::HashMap<String, String> = raw_translations
            .into_iter()
            .filter_map(|t| {
                let txt = t.translated_text.trim().to_string();
                if txt.is_empty() {
                    None
                } else {
                    Some((t.segment_id, txt))
                }
            })
            .collect();

        let mut translated_segments = Vec::new();

        for source_seg in &transcript.segments {
            let translated_text = map
                .get(&source_seg.id)
                .cloned()
                .unwrap_or_else(|| source_seg.text.clone());

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
