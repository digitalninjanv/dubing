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
}
