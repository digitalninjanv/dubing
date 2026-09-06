use crate::domain::{DomainError, SynthesizedSegment, TranslationSegment, VoiceProfile};
use async_trait::async_trait;
use std::path::Path;

#[async_trait]
pub trait SpeechSynthesizer: Send + Sync {
    async fn synthesize_segment(
        &self,
        segment: &TranslationSegment,
        voice: &VoiceProfile,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError>;

    /// Synthesize standalone text directly with optional speaking style instructions
    async fn synthesize_text(
        &self,
        text: &str,
        voice: &VoiceProfile,
        style_instruction: Option<&str>,
        output_path: &Path,
    ) -> Result<SynthesizedSegment, DomainError> {
        let dummy = TranslationSegment {
            segment_id: "direct_tts".to_string(),
            speaker_id: None,
            source_start_ms: 0,
            source_end_ms: 0,
            source_text: text.to_string(),
            translated_text: text.to_string(),
        };
        let mut v = voice.clone();
        if let Some(s) = style_instruction {
            v.style = Some(s.to_string());
        }
        self.synthesize_segment(&dummy, &v, output_path).await
    }
}
