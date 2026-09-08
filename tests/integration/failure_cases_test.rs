use audiodub::application::ports::AudioEngine;
use audiodub::domain::{
    DomainError, LanguageId, LanguageRegistry, SynthesizedSegment, TranscriptSegment,
};
use audiodub::infrastructure::ffmpeg::FfmpegAudioEngine;
use audiodub::infrastructure::gemini::GeminiClient;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_zero_byte_audio_rejected() {
    let dir = tempdir().unwrap();
    let empty_file = dir.path().join("empty.mp3");
    File::create(&empty_file).unwrap();

    let engine = FfmpegAudioEngine::new();
    let result = engine
        .inspect_and_validate(&empty_file, 100 * 1024 * 1024)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, DomainError::InvalidAudio(_)),
        "Expected InvalidAudio error, got {:?}",
        err
    );
}

#[tokio::test]
async fn test_unsupported_audio_extension_rejected() {
    let dir = tempdir().unwrap();
    let bad_file = dir.path().join("document.pdf");
    {
        let mut f = File::create(&bad_file).unwrap();
        f.write_all(b"%PDF-1.5 test").unwrap();
    }

    let engine = FfmpegAudioEngine::new();
    let result = engine
        .inspect_and_validate(&bad_file, 100 * 1024 * 1024)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, DomainError::UnsupportedFormat(_)),
        "Expected UnsupportedFormat error, got {:?}",
        err
    );
}

#[tokio::test]
async fn test_corrupted_audio_file_rejected() {
    let dir = tempdir().unwrap();
    let corrupt_file = dir.path().join("corrupted.mp3");
    {
        let mut f = File::create(&corrupt_file).unwrap();
        f.write_all(b"NOT_A_VALID_MP3_STREAM_CORRUPT_BYTES")
            .unwrap();
    }

    let engine = FfmpegAudioEngine::new();
    let result = engine
        .inspect_and_validate(&corrupt_file, 100 * 1024 * 1024)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, DomainError::InvalidAudio(_)),
        "Expected InvalidAudio error for corrupted file, got {:?}",
        err
    );
}

#[tokio::test]
async fn test_file_size_exceeds_max_rejected() {
    let dir = tempdir().unwrap();
    let sample_mp3 = dir.path().join("sample_small.mp3");

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("sine=frequency=440:duration=1")
        .arg("-c:a")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg("128k")
        .arg(&sample_mp3)
        .status()
        .expect("Failed to execute ffmpeg");
    assert!(status.success());

    let engine = FfmpegAudioEngine::new();
    // Set max limit to 10 bytes (file is ~17 KB)
    let result = engine.inspect_and_validate(&sample_mp3, 10).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, DomainError::InvalidAudio(_)),
        "Expected InvalidAudio error for file size exceedance, got {:?}",
        err
    );
    assert!(err.to_string().contains("exceeds maximum allowed"));
}

#[test]
fn test_language_validation_edge_cases() {
    let registry = LanguageRegistry::standard();

    // 1. Identical languages
    let res = registry.validate_pair(&LanguageId::new("id"), &LanguageId::new("id"));
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("cannot be identical"));

    // 2. Target language is 'auto'
    let res = registry.validate_pair(&LanguageId::new("en"), &LanguageId::auto());
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("cannot be 'auto'"));

    // 3. Unsupported source language
    let res = registry.validate_pair(&LanguageId::new("klingon"), &LanguageId::new("en"));
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("not supported for transcription"));

    // 4. Unsupported target language
    let res = registry.validate_pair(&LanguageId::new("en"), &LanguageId::new("elvish"));
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("not supported for translation"));

    // 5. Valid pair
    let res = registry.validate_pair(&LanguageId::new("id"), &LanguageId::new("en"));
    assert!(res.is_ok());

    // 6. Valid auto source
    let res = registry.validate_pair(&LanguageId::auto(), &LanguageId::new("ja"));
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_audio_alignment_clamping_and_warning_on_long_speech() {
    let dir = tempdir().unwrap();
    let synth_file = dir.path().join("long_speech.wav");

    // Generate a 2.0-second audio segment
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("sine=frequency=440:duration=2")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(&synth_file)
        .status()
        .expect("Failed to execute ffmpeg");
    assert!(status.success());

    let engine = FfmpegAudioEngine::new();

    // Source slot is only 1.0 second (ratio = 2.0 / 1.0 = 2.0x, which exceeds max 1.25x)
    let source_timeline = vec![TranscriptSegment {
        id: "seg_0001".to_string(),
        speaker_id: Some("Speaker 1".to_string()),
        start_ms: 0,
        end_ms: 1000,
        text: "This translation is significantly longer than original slot".to_string(),
        words: vec![],
    }];

    let synth_segments = vec![SynthesizedSegment {
        segment_id: "seg_0001".to_string(),
        speaker_id: Some("Speaker 1".to_string()),
        path: synth_file,
        duration_ms: 2000,
    }];

    let alignment_res = engine
        .align_segments(dir.path(), &source_timeline, &synth_segments, Some(1000))
        .await
        .expect("Alignment should succeed with quality warnings");

    assert_eq!(alignment_res.aligned_files.len(), 1);
    assert!(
        !alignment_res.quality_warnings.is_empty(),
        "Expected quality warnings for audio exceeding stretch limit"
    );

    let warning_text = &alignment_res.quality_warnings[0];
    assert!(
        warning_text.contains("1.35x")
            || warning_text.contains("1.25x")
            || warning_text.contains("clamped"),
        "Warning should mention clamping: {}",
        warning_text
    );
}

#[tokio::test]
async fn test_gemini_client_authentication_failure() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/test_auth"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Invalid API Key"))
        .mount(&server)
        .await;

    let client = GeminiClient::new("bad-key")
        .with_base_url(server.uri())
        .with_backoffs(vec![0]);

    let http_client = client.http().clone();
    let url = format!("{}/test_auth", server.uri());

    let result = client
        .post_with_retry("Test Auth", || {
            let cli = http_client.clone();
            let u = url.clone();
            async move { cli.post(&u).send().await }
        })
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, DomainError::AuthenticationFailed),
        "Expected AuthenticationFailed, got {:?}",
        err
    );
}

#[tokio::test]
async fn test_gemini_client_transient_retry_and_recovery() {
    let server = MockServer::start().await;

    // First request: 429 Too Many Requests
    // Second request: 200 OK
    Mock::given(method("POST"))
        .and(path("/test_retry"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/test_retry"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"status\":\"ok\"}"))
        .mount(&server)
        .await;

    let client = GeminiClient::new("valid-key")
        .with_base_url(server.uri())
        .with_backoffs(vec![0, 0]);

    let http_client = client.http().clone();
    let url = format!("{}/test_retry", server.uri());

    let response = client
        .post_with_retry("Test Transient", || {
            let cli = http_client.clone();
            let u = url.clone();
            async move { cli.post(&u).send().await }
        })
        .await
        .expect("Request should succeed on second attempt");

    assert_eq!(response.status().as_u16(), 200);
}

#[tokio::test]
async fn test_gemini_client_retry_exhaustion() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/test_exhaust"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = GeminiClient::new("valid-key")
        .with_base_url(server.uri())
        .with_backoffs(vec![0, 0]);

    let http_client = client.http().clone();
    let url = format!("{}/test_exhaust", server.uri());

    let result = client
        .post_with_retry("Test Exhaustion", || {
            let cli = http_client.clone();
            let u = url.clone();
            async move { cli.post(&u).send().await }
        })
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, DomainError::TransientError(_)),
        "Expected TransientError after retry exhaustion, got {:?}",
        err
    );
}

#[tokio::test]
async fn test_gemini_client_retry_status_callback() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/test_rate_limit_cb"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/test_rate_limit_cb"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"status\":\"ok\"}"))
        .mount(&server)
        .await;

    let callback_messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let cb_clone = callback_messages.clone();

    let client = GeminiClient::new("valid-key")
        .with_base_url(server.uri())
        .with_backoffs(vec![1, 0])
        .with_status_callback(std::sync::Arc::new(move |msg: &str| {
            cb_clone.lock().unwrap().push(msg.to_string());
        }));

    let http_client = client.http().clone();
    let url = format!("{}/test_rate_limit_cb", server.uri());

    let response = client
        .post_with_retry("Test 429 Callback", || {
            let cli = http_client.clone();
            let u = url.clone();
            async move { cli.post(&u).send().await }
        })
        .await
        .expect("Request should succeed on retry");

    assert_eq!(response.status().as_u16(), 200);

    let msgs = callback_messages.lock().unwrap().clone();
    assert!(!msgs.is_empty(), "Status callback should have received progress messages");
    assert!(msgs[0].contains("429") || msgs[0].contains("rate limit"), "Message should mention 429/rate limit");
}

#[tokio::test]
async fn test_gemini_synthesizer_fallback_to_25_flash_tts() {
    use audiodub::application::ports::SpeechSynthesizer;
    use audiodub::domain::{TranslationSegment, VoiceProfile};
    use audiodub::infrastructure::gemini::GeminiSynthesizer;
    use base64::Engine;

    let server = MockServer::start().await;

    // 1. Primary model fails (404)
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-3.1-flash-tts-preview:generateContent"))
        .respond_with(ResponseTemplate::new(404).set_body_string("{\"error\": \"Model not found\"}"))
        .mount(&server)
        .await;

    // Minimal 1 second silent WAV data
    let dummy_pcm = vec![0u8; 48000]; // 1 sec of silence at 24kHz mono
    let dummy_wav_base64 = base64::engine::general_purpose::STANDARD.encode(&dummy_pcm);
    let tts_response = serde_json::json!({
        "candidates": [
            {
                "content": {
                    "parts": [
                        {
                            "inlineData": {
                                "mimeType": "audio/wav",
                                "data": dummy_wav_base64
                            }
                        }
                    ]
                }
            }
        ]
    });

    // 2. Fallback model succeeds (200)
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-2.5-flash-preview-tts:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tts_response))
        .mount(&server)
        .await;

    let client = GeminiClient::new("test-key")
        .with_base_url(server.uri())
        .with_backoffs(vec![0]);

    let synth = GeminiSynthesizer::new(client, "gemini-3.1-flash-tts-preview");
    let temp_dir = tempdir().unwrap();
    let out_file = temp_dir.path().join("fallback_test.wav");

    let voice = VoiceProfile {
        id: "puck".to_string(),
        voice_name: "Puck".to_string(),
        language: "en".to_string(),
        style: None,
        speed: 1.0,
    };

    let seg = TranslationSegment {
        segment_id: "seg-1".to_string(),
        speaker_id: None,
        source_start_ms: 0,
        source_end_ms: 1000,
        source_text: "Hello".to_string(),
        translated_text: "Hello".to_string(),
    };

    let res = synth.synthesize_segment(&seg, &voice, &out_file).await;
    assert!(
        res.is_ok(),
        "Fallback to gemini-2.5-flash-preview-tts should succeed: {:?}",
        res.err()
    );
    assert!(out_file.exists());
}

#[tokio::test]
async fn test_gemini_client_test_connection() {
    let server = MockServer::start().await;

    // 1. Success case: returns 200 with models list
    let models_json = serde_json::json!({
        "models": [
            { "name": "models/gemini-3.5-transcribe" },
            { "name": "models/gemini-3.1-flash-lite" },
            { "name": "models/gemini-3.1-flash-tts-preview" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/v1beta/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&models_json))
        .mount(&server)
        .await;

    let client = GeminiClient::new("valid-api-key").with_base_url(server.uri());
    let models = client
        .test_connection()
        .await
        .expect("test_connection should succeed");

    assert_eq!(models.len(), 3);
    assert!(models.contains(&"gemini-3.5-transcribe".to_string()));

    // 2. Failure case: unauthorized 401 returns AuthenticationFailed
    let server_unauth = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1beta/models"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server_unauth)
        .await;

    let client_bad = GeminiClient::new("bad-key").with_base_url(server_unauth.uri());
    let err = client_bad
        .test_connection()
        .await
        .expect_err("test_connection should fail with 401");

    assert_eq!(err, audiodub::domain::DomainError::AuthenticationFailed);
}



