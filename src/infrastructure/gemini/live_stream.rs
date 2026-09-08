use crate::domain::{DomainError, LanguageId, LiveModelChoice, LiveTranscriptUpdate};
use crate::infrastructure::gemini::GeminiClient;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
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
    /// audio streaming.
    ///
    /// Uses official Live Translation configuration for gemini-3.5-live-translate-preview
    /// (translationConfig + input/output transcriptions) per Google AI docs 2026.
    /// Handshake is hardened: dual transcription placement, binary-frame support,
    /// full message logging, 25s timeout.
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

        // Build setup payload. For Live Translate we place transcriptions BOTH at
        // setup top-level (production examples / livekit) AND inside generationConfig
        // (official docs) for maximum compatibility.
        let setup_msg = match config.model_choice {
            LiveModelChoice::Gemini35LiveTranslate => {
                serde_json::json!({
                    "setup": {
                        "model": format!("models/{}", config.model_choice.model_id()),
                        "inputAudioTranscription": {},
                        "outputAudioTranscription": {},
                        "generationConfig": {
                            "responseModalities": ["AUDIO"],
                            "inputAudioTranscription": {},
                            "outputAudioTranscription": {},
                            "translationConfig": {
                                "targetLanguageCode": config.target_lang.as_str(),
                                "echoTargetLanguage": false
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
                            }
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
                            "responseModalities": ["TEXT"]
                        },
                        "systemInstruction": {
                            "parts": [{
                                "text": "You are a real-time speech transcription assistant. Transcribe incoming spoken audio into text immediately as speech occurs. Output only the verbatim transcript."
                            }]
                        }
                    }
                })
            }
        };

        let setup_str = setup_msg.to_string();
        info!("Sending Live API setup message ({} bytes)", setup_str.len());
        debug!("Setup payload: {}", setup_str);

        ws_sender
            .send(Message::Text(setup_str))
            .await
            .map_err(|e| {
                DomainError::TransientError(format!("Failed to send Live API setup message: {}", e))
            })?;

        // Handshake: wait for setupComplete. Handle Text + Binary frames, log everything.
        info!("Waiting for Gemini Live API setupComplete acknowledgment...");
        let mut last_raw: Option<String> = None;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    let _ = ws_sender.close().await;
                    return Err(DomainError::Cancelled);
                }
                msg_res = ws_receiver.next() => {
                    match msg_res {
                        Some(Ok(Message::Text(text))) => {
                            info!("Live API handshake Text frame ({} bytes)", text.len());
                            debug!("Handshake Text: {}", text);
                            last_raw = Some(text.clone());
                            if let Some(result) = Self::process_handshake_payload(&text, &mut ws_sender).await? {
                                if result {
                                    break; // setupComplete received
                                }
                            }
                        }
                        Some(Ok(Message::Binary(bin))) => {
                            // Some clients/servers exchange JSON as binary frames
                            let text = String::from_utf8_lossy(&bin).to_string();
                            info!("Live API handshake BINARY frame ({} bytes)", bin.len());
                            debug!("Handshake binary-as-text: {}", text);
                            last_raw = Some(text.clone());
                            if let Some(result) = Self::process_handshake_payload(&text, &mut ws_sender).await? {
                                if result {
                                    break;
                                }
                            }
                        }
                        Some(Ok(Message::Ping(p))) => {
                            let _ = ws_sender.send(Message::Pong(p)).await;
                        }
                        Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(Message::Close(reason))) => {
                            let detail = last_raw.as_deref().unwrap_or("(no prior payload)");
                            return Err(DomainError::TransientError(format!(
                                "Gemini Live API closed during handshake: {:?}. Last payload: {}",
                                reason, detail
                            )));
                        }
                        Some(Err(e)) => {
                            return Err(DomainError::TransientError(format!(
                                "WebSocket receive error during handshake: {}",
                                e
                            )));
                        }
                        None => {
                            let detail = last_raw.as_deref().unwrap_or("(no prior payload)");
                            return Err(DomainError::TransientError(format!(
                                "Gemini Live API connection closed unexpectedly during handshake. Last payload: {}",
                                detail
                            )));
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(25)) => {
                    let detail = last_raw.as_deref().unwrap_or("(no response received at all)");
                    let _ = ws_sender.close().await;
                    return Err(DomainError::TransientError(format!(
                        "Timed out waiting for Gemini Live API setupComplete (25s). Last server payload: {}. \
Check: (1) API key has Live API / Live Translate access, (2) model gemini-3.5-live-translate-preview is available in your region/tier, (3) network allows WSS to generativelanguage.googleapis.com",
                        detail
                    )));
                }
            }
        }

        info!("Gemini Live API setup complete! Ready for real-time audio streaming.");
        debug!("Streaming audio chunks in real-time...");

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

                maybe_chunk = input_rx.recv() => {
                    match maybe_chunk {
                        Some(raw_chunk) => {
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&raw_chunk);
                            let audio_msg = serde_json::json!({
                                "realtimeInput": {
                                    "audio": {
                                        "mimeType": "audio/pcm;rate=16000",
                                        "data": b64
                                    }
                                }
                            });
                            if let Err(e) = ws_sender.send(Message::Text(audio_msg.to_string())).await {
                                warn!("WebSocket send error: {}", e);
                                break;
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

                msg_res = ws_receiver.next() => {
                    match msg_res {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                                if let Some(err) = parsed.get("error") {
                                    let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown Live API error");
                                    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                                    error!("Gemini Live API error: {} (code {})", msg, code);
                                    if code == 429 {
                                        return Err(DomainError::TransientError(format!("Live API rate limited (429): {}", msg)));
                                    } else {
                                        return Err(DomainError::PermanentApiError(format!("Live API error: {}", msg)));
                                    }
                                }

                                let server_content = parsed
                                    .get("serverContent")
                                    .or_else(|| parsed.get("server_content"));

                                if let Some(server_content) = server_content {
                                    let mut original_text: Option<String> = None;
                                    let mut translated_text: Option<String> = None;

                                    let model_turn = server_content
                                        .get("modelTurn")
                                        .or_else(|| server_content.get("model_turn"));

                                    if let Some(model_turn) = model_turn {
                                        if let Some(parts) = model_turn.get("parts").and_then(|p| p.as_array()) {
                                            for part in parts {
                                                let inline_data = part
                                                    .get("inlineData")
                                                    .or_else(|| part.get("inline_data"));

                                                if let Some(inline_data) = inline_data {
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

                                    let input_trans = server_content
                                        .get("inputTranscription")
                                        .or_else(|| server_content.get("input_transcription"))
                                        .and_then(|t| t.get("text"))
                                        .and_then(|s| s.as_str());
                                    if let Some(in_tx) = input_trans {
                                        let trimmed = in_tx.trim().to_string();
                                        if !trimmed.is_empty() {
                                            original_text = Some(trimmed);
                                        }
                                    }

                                    let output_trans = server_content
                                        .get("outputTranscription")
                                        .or_else(|| server_content.get("output_transcription"))
                                        .and_then(|t| t.get("text"))
                                        .and_then(|s| s.as_str());
                                    if let Some(out_tx) = output_trans {
                                        let trimmed = out_tx.trim().to_string();
                                        if !trimmed.is_empty() {
                                            translated_text = Some(trimmed);
                                        }
                                    }

                                    let is_turn_complete = server_content
                                        .get("turnComplete")
                                        .or_else(|| server_content.get("turn_complete"))
                                        .and_then(|tc| tc.as_bool())
                                        .unwrap_or(false);

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
                            let _ = output_tx.send(bin).await;
                        }
                        Some(Ok(Message::Ping(p))) => {
                            let _ = ws_sender.send(Message::Pong(p)).await;
                        }
                        Some(Ok(Message::Close(reason))) => {
                            info!("WebSocket closed by server: {:?}", reason);
                            break;
                        }
                        Some(Err(e)) => {
                            warn!("WebSocket receive error: {}", e);
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
            }
        }

        info!("Live streaming session completed. Total audio chunks sent: {}", chunks_sent);
        Ok(())
    }

    /// Returns Ok(Some(true)) if setupComplete was found, Ok(Some(false)) if message was handled but not complete,
    /// Ok(None) if not a relevant payload, Err on fatal API error.
    async fn process_handshake_payload(
        text: &str,
        ws_sender: &mut (impl SinkExt<Message> + Unpin),
    ) -> Result<Option<bool>, DomainError> {
        let parsed: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };

        if let Some(err) = parsed.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown Live API error");
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            error!("Gemini Live API handshake rejected: {} (code {})", msg, code);
            let _ = ws_sender.close().await;
            if code == 429 {
                return Err(DomainError::TransientError(format!(
                    "Live API rate limited (429): {}",
                    msg
                )));
            } else {
                return Err(DomainError::PermanentApiError(format!(
                    "Live API setup rejected: {} (code {})",
                    msg, code
                )));
            }
        }

        if parsed.get("setupComplete").or_else(|| parsed.get("setup_complete")).is_some() {
            return Ok(Some(true));
        }

        // Any other message is logged but we keep waiting
        Ok(Some(false))
    }
}
