use audiodub::domain::{
    AudioAppInfo, AudioSourceMode, LiveDubberStatus, LiveModelChoice, LiveTranscriptUpdate,
};

#[test]
fn test_live_model_choice_specifications() {
    assert_eq!(
        LiveModelChoice::Gemini35LiveTranslate.model_id(),
        "gemini-3.5-live-translate-preview"
    );
    assert_eq!(
        LiveModelChoice::Gemini31FlashLive.model_id(),
        "gemini-3.1-flash-live-preview"
    );
    assert_eq!(
        LiveModelChoice::Gemini25FlashNativeAudio.model_id(),
        "gemini-2.5-flash-preview-native-audio-dialog"
    );
    assert_eq!(
        LiveModelChoice::Gemini35TranscribeLive.model_id(),
        "gemini-3.5-transcribe-live"
    );

    // Audio output capability
    assert!(LiveModelChoice::Gemini35LiveTranslate.is_audio_output());
    assert!(LiveModelChoice::Gemini31FlashLive.is_audio_output());
    assert!(LiveModelChoice::Gemini25FlashNativeAudio.is_audio_output());
    assert!(!LiveModelChoice::Gemini35TranscribeLive.is_audio_output());

    // Default must be primary S2S model
    assert_eq!(
        LiveModelChoice::default(),
        LiveModelChoice::Gemini35LiveTranslate
    );
}

#[test]
fn test_audio_source_mode() {
    assert_eq!(AudioSourceMode::default(), AudioSourceMode::BrowserYouTube);
    assert!(AudioSourceMode::BrowserYouTube
        .display_name()
        .contains("Silences Original Audio"));
    assert!(AudioSourceMode::SystemDesktop
        .display_name()
        .contains("System Desktop"));
    assert!(AudioSourceMode::Microphone
        .display_name()
        .contains("Microphone"));
}

#[test]
fn test_live_dubber_status_lifecycle() {
    assert_eq!(LiveDubberStatus::Idle.display_status(), "Ready");
    assert!(!LiveDubberStatus::Idle.is_active());
    assert!(!LiveDubberStatus::Stopped.is_active());

    assert!(LiveDubberStatus::Initializing.is_active());
    assert!(LiveDubberStatus::RoutingAudio.is_active());
    assert!(LiveDubberStatus::Streaming.is_active());
    assert!(LiveDubberStatus::Translating.is_active());
    assert!(LiveDubberStatus::Playing.is_active());

    let err_status = LiveDubberStatus::Error("Connection dropped".to_string());
    assert!(!err_status.is_active());
    assert_eq!(err_status.display_status(), "Error Encountered");
}

#[test]
fn test_transcript_update_serialization() {
    let update = LiveTranscriptUpdate {
        original_chunk: Some("Welcome to our channel".to_string()),
        translated_chunk: Some("Selamat datang di saluran kami".to_string()),
        is_turn_complete: true,
        timestamp_ms: 1725750000000,
    };

    let serialized = serde_json::to_string(&update).expect("Serialization failed");
    let deserialized: LiveTranscriptUpdate =
        serde_json::from_str(&serialized).expect("Deserialization failed");

    assert_eq!(update, deserialized);
}

#[test]
fn test_audio_app_info_representation() {
    let app = AudioAppInfo {
        sink_input_id: 42,
        application_name: "Google Chrome".to_string(),
        binary_name: "chrome".to_string(),
        media_name: Some("YouTube - Rust in 100 Seconds".to_string()),
    };

    assert_eq!(app.sink_input_id, 42);
    assert_eq!(app.application_name, "Google Chrome");
    assert_eq!(
        app.media_name.as_deref(),
        Some("YouTube - Rust in 100 Seconds")
    );
}
