use crate::domain::{DomainError, LanguageId, LiveModelChoice, LiveTranscriptUpdate};
use crate::infrastructure::gemini::GeminiClient;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct LiveStreamSessionConfig<'a> {
    pub model_choice: LiveModelChoice,
    pub target_lang: &'a LanguageId,
    pub target_lang_name: &'a str,
    pub voice_name: &'a str,
}

pub struct GeminiLiveStreamer {
    client: GeminiClient,
}

impl GeminiLiveStreamer {
    pub fn new(client: GeminiClient) -> Self {
        Self { client }
    }

    /// Connects to the Gemini Multimodal Live API WebSocket and runs continuous bidirectional
    /// audio streaming:
    /// - Consumes 16kHz PCM chunks from `input_rx`
    /// - Emits synthesized 24kHz PCM chunks to `output_tx`
    /// - Emits real-time transcript updates to `transcript_tx`
    pub async fn run_live_session(
        &self,
        config: LiveStreamSessionConfig<'_>,
        mut input_rx: Receiver<Vec<u8>>,
        output_tx: Sender<Vec<u8>>,
        transcript_tx: Sender<LiveTranscriptUpdate>,
        cancel_token: CancellationToken,
    ) -> Result<(), DomainError> {
        let ws_url = format!(
            "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={}",
            self.client.api_key()
        );

        info!(
            "Connecting to Gemini Multimodal Live API WebSocket (model: {}, target: {})",
            config.model_choice.model_id(),
            config.target_lang.as_str()
        );

        let (ws_stream, _) = connect_async(&ws_url).await.map_err(|e| {
            DomainError::TransientError(format!("Gemini Live API connection failed: {}", e))
        })?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Build official setup payload
        let setup_msg = match config.model_choice {
            LiveModelChoice::Gemini35LiveTranslate => {
                serde_json::json!({
                    "setup": {
                        "model": format!("models/{}", config.model_choice.model_id()),
                        "generationConfig": {
                            "responseModalities": ["AUDIO"],
                            "speechConfig": {
                                "voiceConfig": {
                                    "prebuiltVoiceConfig": {
                                        "voiceName": config.voice_name
                                    }
                                }
                            },
                            "inputAudioTranscription": {},
                            "outputAudioTranscription": {},
                            "translationConfig": {
                                "targetLanguageCode": config.target_lang.as_str(),
                                "echoTargetLanguage": true
                            }
                        }
                    }
                })
            }
            LiveModelChoice::Gemini31FlashLive | LiveModelChoice::Gemini25FlashNativeAudio => {
                serde_json::json!({
                    "setup": {
                        "model": format!("models/{}", config.model_choice.model_id()),
                        "generationConfig": {
                            "responseModalities": ["AUDIO"],
                            "speechConfig": {
                                "voiceConfig": {
                                    "prebuiltVoiceConfig": {
                                        "voiceName": config.voice_name
                                    }
                                }
                            },
                            "inputAudioTranscription": {},
                            "outputAudioTranscription": {}
                        },
                        "systemInstruction": {
                            "parts": [{
                                "text": format!(
                                    "You are a professional real-time simultaneous speech interpreter. Your role is to listen to the incoming audio stream and immediately speak the translation in fluent {} in real-time. Strictly do not engage in conversation, do not summarize, and do not answer questions asked in the audio. Only speak the exact real-time translation with minimum latency.",
                                    config.target_lang_name
                                )
                            }]
                        }
                    }
                })
            }
            LiveModelChoice::Gemini35TranscribeLive => {
                serde_json::json!({
                    "setup": {
                        "model": format!("models/{}", config.model_choice.model_id()),
                        "generationConfig": {
                            "responseModalities": ["TEXT"],
                            "inputAudioTranscription": {}
                        }
                    }
                })
            }
        };

        ws_sender
            .send(Message::Text(setup_msg.to_string()))
            .await
            .map_err(|e| {
                DomainError::TransientError(format!("Failed to send Live API setup message: {}", e))
            })?;

        // A successful TCP/WebSocket upgrade is not a successful Live API session.  The server
        // validates the model and setup payload asynchronously, so do not send audio until its
        // setup acknowledgement has arrived.
        let setup_response = tokio::time::timeout(Duration::from_secs(15), ws_receiver.next())
            .await
            .map_err(|_| {
                DomainError::TransientError(
                    "Timed out waiting for Gemini Live API setup acknowledgement".to_string(),
                )
            })?;
        match setup_response {
            Some(Ok(Message::Text(text))) => {
                let parsed: Value = serde_json::from_str(&text).map_err(|_| {
                    DomainError::PermanentApiError(
                        "Gemini Live API returned an invalid setup response".to_string(),
                    )
                })?;
                if let Some(err) = parsed.get("error") {
                    let message = err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Unknown setup error");
                    let code = err.get("code").and_then(Value::as_i64).unwrap_or_default();
                    return Err(classify_live_api_error(code, message));
                }
                if parsed.get("setupComplete").is_none() {
                    return Err(DomainError::PermanentApiError(
                        "Gemini Live API did not acknowledge the requested model setup."
                            .to_string(),
                    ));
                }
            }
            Some(Ok(Message::Close(reason))) => {
                return Err(DomainError::TransientError(format!(
                    "Gemini Live API closed during setup: {:?}",
                    reason
                )))
            }
            Some(Ok(_)) => {
                return Err(DomainError::PermanentApiError(
                    "Gemini Live API returned an unexpected setup response.".to_string(),
                ))
            }
            Some(Err(error)) => {
                return Err(DomainError::TransientError(format!(
                    "Gemini Live API setup receive failed: {}",
                    error
                )))
            }
            None => {
                return Err(DomainError::TransientError(
                    "Gemini Live API closed before setup completed.".to_string(),
                ))
            }
        }

        debug!("Gemini Live API setup acknowledged. Streaming audio chunks in real-time...");

        let mut chunks_sent: u64 = 0;

        loop {
            if cancel_token.is_cancelled() {
                debug!("Live session cancelled; closing WebSocket");
                let _ = ws_sender.close().await;
                break;
            }

            tokio::select! {
                _ = cancel_token.cancelled() => {
                    let _ = ws_sender.close().await;
                    break;
                }

                // Forward incoming 16kHz PCM audio to Gemini Live API
                maybe_chunk = input_rx.recv() => {
                    match maybe_chunk {
                        Some(raw_chunk) => {
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&raw_chunk);
                            let audio_msg = serde_json::json!({
                                "realtimeInput": {
                                    "mediaChunks": [
                                        {
                                            "mimeType": "audio/pcm;rate=16000",
                                            "data": b64
                                        }
                                    ]
                                }
                            });
                            if let Err(e) = ws_sender.send(Message::Text(audio_msg.to_string())).await {
                                warn!("WebSocket send error: {}", e);
                                return Err(DomainError::TransientError(format!("Gemini Live API send failed: {}", e)));
                            }
                            chunks_sent += 1;
                            if chunks_sent.is_multiple_of(50) {
                                debug!("Live Streamed {} chunks to Gemini", chunks_sent);
                            }
                        }
                        None => {
                            debug!("Input stream ended; finishing session");
                            break;
                        }
                    }
                }

                // Handle incoming server responses from Gemini
                msg_res = ws_receiver.next() => {
                    match msg_res {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                                // Check for API errors
                                if let Some(err) = parsed.get("error") {
                                    let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown Live API error");
                                    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                                    error!("Gemini Live API error: {} (code {})", msg, code);
                                    return Err(classify_live_api_error(code, msg));
                                }

                                if let Some(server_content) = parsed.get("serverContent") {
                                    let mut original_text: Option<String> = None;
                                    let mut translated_text: Option<String> = None;

                                    // 1. Audio synthesis parts
                                    if let Some(model_turn) = server_content.get("modelTurn") {
                                        if let Some(parts) = model_turn.get("parts").and_then(|p| p.as_array()) {
                                            for part in parts {
                                                if let Some(inline_data) = part.get("inlineData") {
                                                    if let Some(b64_audio) = inline_data.get("data").and_then(|d| d.as_str()) {
                                                        if let Ok(audio_bytes) = base64::engine::general_purpose::STANDARD.decode(b64_audio.trim()) {
                                                            let _ = output_tx.send(audio_bytes).await;
                                                        }
                                                    }
                                                }
                                                if let Some(txt) = part.get("text").and_then(|s| s.as_str()) {
                                                    let trimmed = txt.trim().to_string();
                                                    if !trimmed.is_empty() {
                                                        translated_text = Some(trimmed);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // 2. Input transcription
                                    if let Some(input_tx) = server_content.get("inputTranscription").and_then(|t| t.get("text")).and_then(|s| s.as_str()) {
                                        let trimmed = input_tx.trim().to_string();
                                        if !trimmed.is_empty() {
                                            original_text = Some(trimmed);
                                        }
                                    }

                                    // 3. Output transcription
                                    if let Some(output_tx_str) = server_content.get("outputTranscription").and_then(|t| t.get("text")).and_then(|s| s.as_str()) {
                                        let trimmed = output_tx_str.trim().to_string();
                                        if !trimmed.is_empty() {
                                            translated_text = Some(trimmed);
                                        }
                                    }

                                    let is_turn_complete = server_content.get("turnComplete").and_then(|tc| tc.as_bool()).unwrap_or(false);

                                    if original_text.is_some() || translated_text.is_some() || is_turn_complete {
                                        let now_ms = SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis() as u64;

                                        let _ = transcript_tx.send(LiveTranscriptUpdate {
                                            original_chunk: original_text,
                                            translated_chunk: translated_text,
                                            is_turn_complete,
                                            timestamp_ms: now_ms,
                                        }).await;
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Binary(bin))) => {
                            // Direct PCM output if sent in binary mode
                            let _ = output_tx.send(bin).await;
                        }
                        Some(Ok(Message::Ping(p))) => {
                            let _ = ws_sender.send(Message::Pong(p)).await;
                        }
                        Some(Ok(Message::Close(reason))) => {
                            info!("WebSocket closed by server: {:?}", reason);
                            return Err(DomainError::TransientError(format!("Gemini Live API closed unexpectedly: {:?}", reason)));
                        }
                        Some(Err(e)) => {
                            warn!("WebSocket receive error: {}", e);
                            return Err(DomainError::TransientError(format!("Gemini Live API receive failed: {}", e)));
                        }
                        None => {
                            return Err(DomainError::TransientError("Gemini Live API connection ended unexpectedly.".to_string()));
                        }
                        _ => {}
                    }
                }
            }
        }

        info!(
            "Live streaming session completed. Total audio chunks sent: {}",
            chunks_sent
        );
        Ok(())
    }
}

fn classify_live_api_error(code: i64, message: &str) -> DomainError {
    match code {
        429 | 500..=599 => {
            DomainError::TransientError(format!("Gemini Live API error ({}): {}", code, message))
        }
        401 | 403 => DomainError::AuthenticationFailed,
        _ => {
            DomainError::PermanentApiError(format!("Gemini Live API error ({}): {}", code, message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::classify_live_api_error;
    use crate::domain::DomainError;

    #[test]
    fn classifies_authentication_and_retryable_live_api_errors() {
        assert_eq!(
            classify_live_api_error(401, "invalid key"),
            DomainError::AuthenticationFailed
        );
        assert!(matches!(
            classify_live_api_error(429, "quota"),
            DomainError::TransientError(_)
        ));
        assert!(matches!(
            classify_live_api_error(503, "unavailable"),
            DomainError::TransientError(_)
        ));
        assert!(matches!(
            classify_live_api_error(400, "bad setup"),
            DomainError::PermanentApiError(_)
        ));
    }
}
