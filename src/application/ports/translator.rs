use crate::domain::{DomainError, LanguageId, Transcript, TranslatedDocument, TranslationTone};
use async_trait::async_trait;

#[async_trait]
pub trait TextTranslator: Send + Sync {
    async fn translate(
        &self,
        transcript: &Transcript,
        target_lang: &LanguageId,
        tone: TranslationTone,
    ) -> Result<TranslatedDocument, DomainError>;
}
