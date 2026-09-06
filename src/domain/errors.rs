use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DomainError {
    #[error("Audio file is invalid or empty: {0}")]
    InvalidAudio(String),

    #[error("Unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("Language '{0}' is not supported for {1}")]
    UnsupportedLanguage(String, String),

    #[error("API authentication failed: invalid or missing API key")]
    AuthenticationFailed,

    #[error("Transient service error: {0}")]
    TransientError(String),

    #[error("Permanent API error: {0}")]
    PermanentApiError(String),

    #[error("Alignment failure: {0}")]
    AlignmentError(String),

    #[error("Export failure: {0}")]
    ExportError(String),

    #[error("Job was cancelled by user")]
    Cancelled,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl DomainError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, DomainError::TransientError(_))
    }

    pub fn is_permanent(&self) -> bool {
        !self.is_retryable() && !matches!(self, DomainError::Cancelled)
    }

    pub fn human_title(&self) -> &'static str {
        match self {
            DomainError::InvalidAudio(_) => "Invalid Audio File",
            DomainError::UnsupportedFormat(_) => "Unsupported Audio Format",
            DomainError::UnsupportedLanguage(_, _) => "Language Not Supported",
            DomainError::AuthenticationFailed => "Authentication Error",
            DomainError::TransientError(_) => "Temporary Service Issue",
            DomainError::PermanentApiError(_) => "AI Service Error",
            DomainError::AlignmentError(_) => "Audio Alignment Issue",
            DomainError::ExportError(_) => "Audio Export Failed",
            DomainError::Cancelled => "Job Cancelled",
            DomainError::Internal(_) => "System Error",
        }
    }
}
