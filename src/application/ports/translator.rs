use crate::domain::{DomainError, LanguageId, Transcript, TranslatedDocument, TranslationTone};
use async_trait::async_trait;

#[async_trait]
pub trait TextTranslator: Send + Sync {
    /// Stable non-secret identity of the translation implementation and model candidates.
    fn cache_identity(&self) -> String {
        "unknown-translator".to_string()
    }
    async fn translate(
        &self,
        transcript: &Transcript,
        target_lang: &LanguageId,
        tone: TranslationTone,
    ) -> Result<TranslatedDocument, DomainError>;
}
