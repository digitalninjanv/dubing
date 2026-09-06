use crate::domain::{AudioDocument, DomainError, LanguageId, Transcript};
use async_trait::async_trait;

#[async_trait]
pub trait SpeechTranscriber: Send + Sync {
    async fn transcribe(
        &self,
        audio: &AudioDocument,
        source_hint: &LanguageId,
    ) -> Result<Transcript, DomainError>;
}
