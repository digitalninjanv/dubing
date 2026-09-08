//! Shared Gemini Live API wire protocol (`BidiGenerateContent` over WebSocket).
//!
//! Authoritative reference: `ai.google.dev/api/live`
//! (`BidiGenerateContentSetup` proto). Key facts encoded here:
//! - `inputAudioTranscription` / `outputAudioTranscription` are **top-level
//!   `setup` fields**, NOT `generationConfig` fields. Sending them inside
//!   `generationConfig` makes the server close the handshake with
//!   `Invalid JSON payload ... Unknown name at 'setup.generation_config'`.
//! - `translationConfig` / `speechConfig` / `responseModalities` live **inside**
//!   `generationConfig`.
//! - Audio in: `{"realtimeInput": {"audio": {"data": b64, "mimeType":
//!   "audio/pcm;rate=16000"}}}` — raw 16-bit mono PCM @16kHz, ~100ms chunks.
//! - Audio out: `serverContent.modelTurn.parts[].inlineData.data` (24kHz PCM).
//! - Turn lifecycle: `serverContent.turnComplete`, `serverContent.interrupted`
//!   (flush playback), top-level `goAway` and `sessionResumptionUpdate`.

use crate::domain::{DomainError, LiveModelChoice};
use base64::Engine;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// Parameters for building a Live session `{"setup": ...}` message.
pub struct LiveSetupParams<'a> {
    pub model_choice: LiveModelChoice,
    pub target_lang: &'a str,
    pub target_lang_name: &'a str,
    pub voice_name: &'a str,
    /// Resume handle from a previous `sessionResumptionUpdate`, if any.
    pub resume_handle: Option<&'a str>,
}

/// Build the `{"setup": ...}` message for the given model.
///
/// Placement rules (see module docs): transcriptions at top level,
/// translation/speech config inside `generationConfig`.
pub fn build_setup_message(p: &LiveSetupParams<'_>) -> Value {
    let model = format!("models/{}", p.model_choice.model_id());
    let mut setup = match p.model_choice {
        LiveModelChoice::Gemini35LiveTranslate => serde_json::json!({
            "model": model,
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "translationConfig": {
                    "targetLanguageCode": p.target_lang,
                    "echoTargetLanguage": false
                }
            },
            "inputAudioTranscription": {},
            "outputAudioTranscription": {}
        }),
        LiveModelChoice::Gemini31FlashLive | LiveModelChoice::Gemini25FlashNativeAudio => {
            serde_json::json!({
                "model": model,
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": {
                        "voiceConfig": {
                            "prebuiltVoiceConfig": {
                                "voiceName": p.voice_name
                            }
                        }
                    }
                },
                "systemInstruction": {
                    "parts": [{
                        "text": format!(
                            "You are a professional real-time simultaneous speech interpreter. Your role is to listen to the incoming audio stream and immediately speak the translation in fluent {} in real-time. Strictly do not engage in conversation, do not summarize, and do not answer questions asked in the audio. Only speak the exact real-time translation with minimum latency.",
                            p.target_lang_name
                        )
                    }]
                },
                "inputAudioTranscription": {},
                "outputAudioTranscription": {}
            })
        }
        LiveModelChoice::Gemini35TranscribeLive => serde_json::json!({
            "model": model,
            "generationConfig": {
                "responseModalities": ["TEXT"]
            },
            "inputAudioTranscription": {},
            "systemInstruction": {
                "parts": [{
                    "text": "You are a real-time speech transcription assistant. Transcribe incoming spoken audio into text immediately as speech occurs. Output only the verbatim transcript."
                }]
            }
        }),
    };

    if let Some(handle) = p.resume_handle {
        if let Some(obj) = setup.as_object_mut() {
            obj.insert(
                "sessionResumption".to_string(),
                serde_json::json!({ "handle": handle }),
            );
        }
    }

    serde_json::json!({ "setup": setup })
}

/// Validate setup placement **before** sending: fail fast locally instead of
/// waiting for the server to close the handshake with `Invalid`.
pub fn validate_setup_message(msg: &Value) -> Result<(), String> {
    let setup = msg
        .get("setup")
        .ok_or_else(|| "missing top-level 'setup' object".to_string())?;
    setup
        .get("model")
        .and_then(|m| m.as_str())
        .filter(|m| !m.is_empty())
        .ok_or_else(|| "missing 'setup.model'".to_string())?;
    let gen = setup
        .get("generationConfig")
        .ok_or_else(|| "missing 'setup.generationConfig'".to_string())?;
    for forbidden in ["inputAudioTranscription", "outputAudioTranscription"] {
        if gen.get(forbidden).is_some() {
            return Err(format!(
                "'{forbidden}' must be a top-level setup field, not inside generationConfig (server rejects with Unknown name)"
            ));
        }
    }
    if setup.get("inputAudioTranscription").is_none() {
        return Err("missing top-level 'setup.inputAudioTranscription'".to_string());
    }
    Ok(())
}

/// Build a `realtimeInput` audio message for one PCM chunk.
pub fn build_audio_message(pcm_chunk: &[u8]) -> Value {
    let b64 = base64::engine::general_purpose::STANDARD.encode(pcm_chunk);
    serde_json::json!({
        "realtimeInput": {
            "audio": {
                "mimeType": "audio/pcm;rate=16000",
                "data": b64
            }
        }
    })
}

/// Server-side error extracted from a message, if present.
pub fn extract_api_error(parsed: &Value) -> Option<(i64, String)> {
    let err = parsed.get("error")?;
    let msg = err
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("Unknown Live API error")
        .to_string();
    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
    Some((code, msg))
}

/// Map a server error to a domain error (429 → retryable).
pub fn api_error_to_domain(code: i64, msg: &str, context: &str) -> DomainError {
    if code == 429 {
        DomainError::TransientError(format!("Live API rate limited (429) {context}: {msg}"))
    } else {
        DomainError::PermanentApiError(format!("Live API error {context}: {msg} (code {code})"))
    }
}

/// Returns true when the payload is a setup-complete acknowledgement.
pub fn is_setup_complete(text: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    if extract_api_error(&parsed).is_some() {
        return false;
    }
    parsed
        .get("setupComplete")
        .or_else(|| parsed.get("setup_complete"))
        .is_some()
}

/// Parsed content of one server message.
#[derive(Debug, Default)]
pub struct ParsedServerMessage {
    pub audio_chunks: Vec<Vec<u8>>,
    pub model_text_parts: Vec<String>,
    pub original_transcript: Option<String>,
    pub translated_transcript: Option<String>,
    pub turn_complete: bool,
    pub interrupted: bool,
    /// Raw `timeLeft` value from `goAway` (e.g. `"30s"`), if present.
    pub go_away_time_left: Option<String>,
    /// `(new_handle, resumable)` from `sessionResumptionUpdate`, if present.
    pub resumption: Option<(String, bool)>,
}

fn non_empty_text(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Parse one server message. Returns `Ok(None)` when the text is not JSON
/// (binary frames are handled by the caller).
pub fn parse_server_message(text: &str) -> Result<Option<ParsedServerMessage>, DomainError> {
    let parsed: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    if let Some((code, msg)) = extract_api_error(&parsed) {
        return Err(api_error_to_domain(code, &msg, "during streaming"));
    }

    let mut out = ParsedServerMessage::default();

    if let Some(time_left) = parsed
        .get("goAway")
        .or_else(|| parsed.get("go_away"))
        .and_then(|g| g.get("timeLeft").or_else(|| g.get("time_left")))
        .and_then(|t| t.as_str())
    {
        out.go_away_time_left = Some(time_left.to_string());
    }

    if let Some(update) = parsed
        .get("sessionResumptionUpdate")
        .or_else(|| parsed.get("session_resumption_update"))
    {
        let handle = update
            .get("newHandle")
            .or_else(|| update.get("new_handle"))
            .and_then(|h| h.as_str())
            .unwrap_or_default()
            .to_string();
        let resumable = update
            .get("resumable")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);
        if !handle.is_empty() {
            out.resumption = Some((handle, resumable));
        }
    }

    let Some(server_content) = parsed
        .get("serverContent")
        .or_else(|| parsed.get("server_content"))
    else {
        return Ok(Some(out));
    };

    if server_content
        .get("interrupted")
        .and_then(|i| i.as_bool())
        .unwrap_or(false)
    {
        out.interrupted = true;
    }

    if let Some(model_turn) = server_content
        .get("modelTurn")
        .or_else(|| server_content.get("model_turn"))
    {
        if let Some(parts) = model_turn.get("parts").and_then(|p| p.as_array()) {
            for part in parts {
                if let Some(b64) = part
                    .get("inlineData")
                    .or_else(|| part.get("inline_data"))
                    .and_then(|d| d.get("data"))
                    .and_then(|d| d.as_str())
                {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.trim())
                    {
                        if !bytes.is_empty() {
                            out.audio_chunks.push(bytes);
                        }
                    }
                }
                if let Some(txt) = non_empty_text(part.get("text").and_then(|s| s.as_str())) {
                    out.model_text_parts.push(txt);
                }
            }
        }
    }

    out.original_transcript = non_empty_text(
        server_content
            .get("inputTranscription")
            .or_else(|| server_content.get("input_transcription"))
            .and_then(|t| t.get("text"))
            .and_then(|s| s.as_str()),
    );
    let output_trans = non_empty_text(
        server_content
            .get("outputTranscription")
            .or_else(|| server_content.get("output_transcription"))
            .and_then(|t| t.get("text"))
            .and_then(|s| s.as_str()),
    );
    // Prefer explicit output transcription; fall back to model text parts.
    out.translated_transcript = output_trans.or_else(|| {
        if out.model_text_parts.is_empty() {
            None
        } else {
            Some(out.model_text_parts.join(" "))
        }
    });

    out.turn_complete = server_content
        .get("turnComplete")
        .or_else(|| server_content.get("turn_complete"))
        .and_then(|tc| tc.as_bool())
        .unwrap_or(false);

    Ok(Some(out))
}

/// Current time in millis, for transcript event timestamps.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::LiveModelChoice;

    fn params() -> (String, String, String) {
        (
            "id".to_string(),
            "Indonesian".to_string(),
            "Aoede".to_string(),
        )
    }

    #[test]
    fn translate_setup_is_top_level_and_valid() {
        let (lang, name, voice) = params();
        let msg = build_setup_message(&LiveSetupParams {
            model_choice: LiveModelChoice::Gemini35LiveTranslate,
            target_lang: &lang,
            target_lang_name: &name,
            voice_name: &voice,
            resume_handle: None,
        });
        assert!(validate_setup_message(&msg).is_ok());
        let setup = &msg["setup"];
        assert!(setup.get("inputAudioTranscription").is_some());
        assert!(setup.get("outputAudioTranscription").is_some());
        assert!(setup["generationConfig"].get("translationConfig").is_some());
        assert!(setup["generationConfig"]
            .get("inputAudioTranscription")
            .is_none());
    }

    #[test]
    fn validator_rejects_generation_config_transcriptions() {
        let bad = serde_json::json!({
            "setup": {
                "model": "models/gemini-3.5-live-translate-preview",
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "inputAudioTranscription": {}
                }
            }
        });
        let err = validate_setup_message(&bad).expect_err("must reject");
        assert!(err.contains("top-level"), "unexpected: {err}");
    }

    #[test]
    fn resume_handle_is_top_level() {
        let (lang, name, voice) = params();
        let msg = build_setup_message(&LiveSetupParams {
            model_choice: LiveModelChoice::Gemini35LiveTranslate,
            target_lang: &lang,
            target_lang_name: &name,
            voice_name: &voice,
            resume_handle: Some("handle-123"),
        });
        assert_eq!(msg["setup"]["sessionResumption"]["handle"], "handle-123");
        assert!(validate_setup_message(&msg).is_ok());
    }

    #[test]
    fn parses_audio_and_transcripts_and_turn() {
        let audio_b64 = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3, 4]);
        let text = format!(
            r#"{{"serverContent": {{"modelTurn": {{"parts": [{{"inlineData": {{"data": "{audio_b64}"}}}}, {{"text": "halo"}}]}}, "inputTranscription": {{"text": "hello"}}, "outputTranscription": {{"text": "halo dunia"}}, "turnComplete": true}}}}"#
        );
        let parsed = parse_server_message(&text).expect("parse").expect("some");
        assert_eq!(parsed.audio_chunks.len(), 1);
        assert_eq!(parsed.original_transcript.as_deref(), Some("hello"));
        assert_eq!(parsed.translated_transcript.as_deref(), Some("halo dunia"));
        assert!(parsed.turn_complete);
        assert!(!parsed.interrupted);
    }

    #[test]
    fn parses_goaway_resumption_and_interrupted() {
        let go = parse_server_message(r#"{"goAway": {"timeLeft": "30s"}}"#)
            .expect("parse")
            .expect("some");
        assert_eq!(go.go_away_time_left.as_deref(), Some("30s"));

        let rs = parse_server_message(
            r#"{"sessionResumptionUpdate": {"newHandle": "h1", "resumable": true}}"#,
        )
        .expect("parse")
        .expect("some");
        assert_eq!(rs.resumption, Some(("h1".to_string(), true)));

        let intr = parse_server_message(r#"{"serverContent": {"interrupted": true}}"#)
            .expect("parse")
            .expect("some");
        assert!(intr.interrupted);
    }

    #[test]
    fn api_error_maps_429_to_transient() {
        let err = parse_server_message(r#"{"error": {"code": 429, "message": "quota"}}"#);
        assert!(matches!(err, Err(DomainError::TransientError(_))));
        let err = parse_server_message(r#"{"error": {"code": 400, "message": "bad"}}"#);
        assert!(matches!(err, Err(DomainError::PermanentApiError(_))));
    }
}
