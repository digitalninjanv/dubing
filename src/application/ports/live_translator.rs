use crate::domain::{DomainError, LanguageId};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct LiveTranslationResult {
    pub raw_pcm_path: PathBuf,
    pub input_transcripts: Vec<String>,
    pub output_transcripts: Vec<String>,
}

#[async_trait]
pub trait LiveSpeechTranslator: Send + Sync {
    async fn translate_speech(
        &self,
        input_path: &Path,
        target_lang: &LanguageId,
        work_dir: &Path,
        cancel_token: &CancellationToken,
    ) -> Result<LiveTranslationResult, DomainError>;
}
