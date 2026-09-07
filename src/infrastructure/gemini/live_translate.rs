use crate::application::ports::{LiveSpeechTranslator, LiveTranslationResult};
use crate::domain::{DomainError, LanguageId};
use crate::infrastructure::gemini::GeminiClient;
use async_trait::async_trait;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

pub struct GeminiLiveTranslator {
    client: GeminiClient,
    model_name: String,
}

impl GeminiLiveTranslator {
    pub fn new(client: GeminiClient, model_name: impl Into<String>) -> Self {
        Self {
            client,
            model_name: model_name.into(),
        }
    }

    /// Converts input media file to raw 16-bit 16kHz PCM (mono, little-endian) using FFmpeg.
    fn convert_to_pcm_16k(&self, input_path: &Path, output_pcm: &Path) -> Result<(), DomainError> {
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                input_path.to_str().unwrap_or_default(),
                "-vn",
                "-f",
                "s16le",
                "-acodec",
                "pcm_s16le",
                "-ac",
                "1",
                "-ar",
                "16000",
                output_pcm.to_str().unwrap_or_default(),
            ])
            .output()
            .map_err(|e| DomainError::Internal(format!("Failed to spawn ffmpeg: {}", e)))?;

        if !status.status.success() {
            return Err(DomainError::Internal(format!(
                "FFmpeg PCM conversion failed: {}",
                String::from_utf8_lossy(&status.stderr)
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl LiveSpeechTranslator for GeminiLiveTranslator {
    async fn translate_speech(
        &self,
        input_path: &Path,
        target_lang: &LanguageId,
        work_dir: &Path,
        cancel_token: &CancellationToken,
    ) -> Result<LiveTranslationResult, DomainError> {
        if cancel_token.is_cancelled() {
            return Err(DomainError::Cancelled);
        }

        let input_pcm_path = work_dir.join("input_16k.pcm");
        let output_pcm_path = work_dir.join("output_24k.pcm");

        info!(
            "Converting audio to 16kHz PCM for Gemini Live Translate: {}",
            input_path.display()
        );
        self.convert_to_pcm_16k(input_path, &input_pcm_path)?;

        let pcm_bytes = std::fs::read(&input_pcm_path)
            .map_err(|e| DomainError::Internal(format!("Failed to read raw PCM file: {}", e)))?;

        if pcm_bytes.is_empty() {
            return Err(DomainError::InvalidAudio(
                "Extracted PCM audio is empty".to_string(),
            ));
        }

        let ws_url = format!(
            "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={}",
            self.client.api_key()
        );

        info!(
            "Connecting to Gemini Live API WebSocket ({}) for target language: {}",
            self.model_name,
            target_lang.as_str()
        );

        let (ws_stream, _) = connect_async(&ws_url).await.map_err(|e| {
            DomainError::TransientError(format!("Gemini Live API WebSocket connection failed: {}", e))
        })?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // 1. Send Setup Message
        let setup_msg = serde_json::json!({
            "setup": {
                "model": format!("models/{}", self.model_name),
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "inputAudioTranscription": {},
                    "outputAudioTranscription": {},
                    "translationConfig": {
                        "targetLanguageCode": target_lang.as_str(),
                        "echoTargetLanguage": true
                    }
                }
            }
        });

        ws_sender
            .send(Message::Text(setup_msg.to_string()))
            .await
            .map_err(|e| {
                DomainError::TransientError(format!("Failed to send setup message: {}", e))
            })?;

        debug!("Gemini Live Translate setup message delivered");

        let mut output_file = File::create(&output_pcm_path).map_err(|e| {
            DomainError::Internal(format!("Failed to create output PCM file: {}", e))
        })?;

        let mut input_transcripts: Vec<String> = Vec::new();
        let mut output_transcripts: Vec<String> = Vec::new();

        // Chunking: 100ms chunks (3200 bytes at 16kHz 16-bit mono)
        const CHUNK_SIZE: usize = 3200;
        let chunks: Vec<Vec<u8>> = pcm_bytes.chunks(CHUNK_SIZE).map(|c| c.to_vec()).collect();
        let total_chunks = chunks.len();

        let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<Option<Vec<u8>>>(16);

        // Spawn sender streamer
        let cancel_sender = cancel_token.clone();
        tokio::spawn(async move {
            for chunk in chunks {
                if cancel_sender.is_cancelled() {
                    break;
                }
                if chunk_tx.send(Some(chunk)).await.is_err() {
                    break;
                }
                // Gentle throttle: 60ms pacing between 100ms chunks to deliver smoothly
                tokio::time::sleep(Duration::from_millis(60)).await;
            }
            let _ = chunk_tx.send(None).await;
        });

        let mut sender_done = false;
        let mut turn_completed = false;
        let mut chunks_sent = 0;

        // Unified bidirectional processing loop
        while !turn_completed {
            if cancel_token.is_cancelled() {
                let _ = ws_sender.close().await;
                return Err(DomainError::Cancelled);
            }

            tokio::select! {
                // Outgoing chunks
                maybe_chunk = chunk_rx.recv(), if !sender_done => {
                    match maybe_chunk {
                        Some(Some(raw_chunk)) => {
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&raw_chunk);
                            let audio_msg = serde_json::json!({
                                "realtimeInput": {
                                    "audio": {
                                        "data": b64,
                                        "mimeType": "audio/pcm;rate=16000"
                                    }
                                }
                            });
                            if let Err(e) = ws_sender.send(Message::Text(audio_msg.to_string())).await {
                                warn!("Failed sending audio chunk to Live API: {}", e);
                                break;
                            }
                            chunks_sent += 1;
                            if chunks_sent % 20 == 0 || chunks_sent == total_chunks {
                                debug!("Live Translate streamed {}/{} chunks ({:.1}%)", chunks_sent, total_chunks, (chunks_sent as f32 / total_chunks as f32) * 100.0);
                            }
                        }
                        Some(None) | None => {
                            sender_done = true;
                            debug!("Audio streaming finished ({} chunks sent). Signaling client turn complete.", chunks_sent);
                            let end_turn_msg = serde_json::json!({
                                "clientContent": {
                                    "turns": [
                                        {
                                            "role": "user",
                                            "parts": []
                                        }
                                    ],
                                    "turnComplete": true
                                }
                            });
                            let _ = ws_sender.send(Message::Text(end_turn_msg.to_string())).await;
                        }
                    }
                }

                // Incoming server responses
                msg_res = ws_receiver.next() => {
                    match msg_res {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                                // Check for API errors
                                if let Some(err) = parsed.get("error") {
                                    let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown Live API error");
                                    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                                    error!("Gemini Live API error response: {} (code {})", msg, code);
                                    if code == 429 {
                                        return Err(DomainError::TransientError(format!("Gemini Live API rate limited (429): {}", msg)));
                                    } else {
                                        return Err(DomainError::PermanentApiError(format!("Gemini Live API error: {}", msg)));
                                    }
                                }

                                if let Some(server_content) = parsed.get("serverContent") {
                                    // 1. Extract audio parts
                                    if let Some(model_turn) = server_content.get("modelTurn") {
                                        if let Some(parts) = model_turn.get("parts").and_then(|p| p.as_array()) {
                                            for part in parts {
                                                if let Some(inline_data) = part.get("inlineData") {
                                                    if let Some(b64_audio) = inline_data.get("data").and_then(|d| d.as_str()) {
                                                        if let Ok(audio_bytes) = base64::engine::general_purpose::STANDARD.decode(b64_audio) {
                                                            let _ = output_file.write_all(&audio_bytes);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // 2. Extract input transcription
                                    if let Some(input_tx) = server_content.get("inputTranscription").and_then(|t| t.get("text")).and_then(|s| s.as_str()) {
                                        let trimmed = input_tx.trim();
                                        if !trimmed.is_empty() && !input_transcripts.iter().any(|t| t == trimmed) {
                                            input_transcripts.push(trimmed.to_string());
                                        }
                                    }

                                    // 3. Extract output transcription
                                    if let Some(output_tx) = server_content.get("outputTranscription").and_then(|t| t.get("text")).and_then(|s| s.as_str()) {
                                        let trimmed = output_tx.trim();
                                        if !trimmed.is_empty() && !output_transcripts.iter().any(|t| t == trimmed) {
                                            output_transcripts.push(trimmed.to_string());
                                        }
                                    }

                                    // 4. Check for turn completion
                                    if server_content.get("turnComplete").and_then(|tc| tc.as_bool()) == Some(true) {
                                        debug!("Live Translate turnComplete received from server");
                                        if sender_done {
                                            turn_completed = true;
                                        }
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Binary(bin))) => {
                            // Direct binary PCM payload if emitted in raw mode
                            let _ = output_file.write_all(&bin);
                        }
                        Some(Ok(Message::Close(reason))) => {
                            debug!("WebSocket closed by server: {:?}", reason);
                            break;
                        }
                        Some(Err(e)) => {
                            warn!("WebSocket receive error: {}", e);
                            break;
                        }
                        None => {
                            // End of stream
                            break;
                        }
                        _ => {}
                    }
                }

                // Inactivity timeout: 12 seconds after sender completes
                _ = tokio::time::sleep(Duration::from_secs(12)), if sender_done => {
                    info!("Live Translate receive window completed after sender finished");
                    turn_completed = true;
                }
            }
        }

        let _ = output_file.flush();
        let metadata = std::fs::metadata(&output_pcm_path).map_err(|e| {
            DomainError::Internal(format!("Failed to query output PCM metadata: {}", e))
        })?;

        if metadata.len() == 0 {
            return Err(DomainError::TransientError(
                "Gemini Live Translate did not return synthesized audio bytes".to_string(),
            ));
        }

        info!(
            "Live Translate complete: collected {} bytes of 24kHz raw PCM",
            metadata.len()
        );

        Ok(LiveTranslationResult {
            raw_pcm_path: output_pcm_path,
            input_transcripts,
            output_transcripts,
        })
    }
}
