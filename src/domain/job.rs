use super::audio::AudioDocument;
use super::language::LanguageId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(String);

impl JobId {
    pub fn generate() -> Self {
        let now = Utc::now().format("%Y%m%d_%H%M%S");
        let id = Uuid::new_v4().simple().to_string();
        Self(format!("job_{}_{}", now, &id[..8]))
    }

    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DubbingEngine {
    #[default]
    Studio,
    LiveTranslate,
}

impl DubbingEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Studio => "studio",
            Self::LiveTranslate => "live",
        }
    }
}

impl std::fmt::Display for DubbingEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Studio => write!(f, "Studio Multi-Stage"),
            Self::LiveTranslate => {
                write!(f, "Live Fast Translate (gemini-3.5-live-translate-preview)")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineStage {
    Idle,
    Validating,
    Uploading,
    Transcribing,
    Translating,
    Synthesizing,
    Aligning,
    Exporting,
    ValidatingOutput,
    Completed,
    FailedRetryable,
    FailedPermanent,
    Cancelled,
}

impl PipelineStage {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::FailedPermanent | Self::Cancelled
        )
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, Self::FailedRetryable | Self::FailedPermanent)
    }

    pub fn can_transition_to(&self, next: &PipelineStage) -> bool {
        // Any non-terminal state can transition to Cancelled or Failure states
        if matches!(
            next,
            PipelineStage::Cancelled
                | PipelineStage::FailedRetryable
                | PipelineStage::FailedPermanent
        ) {
            return !self.is_terminal();
        }

        // Retryable failure can transition back to the failed stage for retry
        if *self == PipelineStage::FailedRetryable {
            return matches!(
                next,
                PipelineStage::Validating
                    | PipelineStage::Uploading
                    | PipelineStage::Transcribing
                    | PipelineStage::Translating
                    | PipelineStage::Synthesizing
                    | PipelineStage::Aligning
                    | PipelineStage::Exporting
                    | PipelineStage::ValidatingOutput
            );
        }

        matches!(
            (self, next),
            (PipelineStage::Idle, PipelineStage::Validating)
                | (PipelineStage::Validating, PipelineStage::Uploading)
                | (PipelineStage::Uploading, PipelineStage::Transcribing)
                | (PipelineStage::Transcribing, PipelineStage::Translating)
                | (PipelineStage::Translating, PipelineStage::Synthesizing)
                | (PipelineStage::Synthesizing, PipelineStage::Aligning)
                | (PipelineStage::Aligning, PipelineStage::Exporting)
                | (PipelineStage::Exporting, PipelineStage::ValidatingOutput)
                | (PipelineStage::ValidatingOutput, PipelineStage::Completed)
        )
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Validating => "Validating audio...",
            Self::Uploading => "Uploading audio to Gemini...",
            Self::Transcribing => "Transcribing speech...",
            Self::Translating => "Translating transcript...",
            Self::Synthesizing => "Generating voice audio...",
            Self::Aligning => "Aligning audio timeline...",
            Self::Exporting => "Exporting final audio...",
            Self::ValidatingOutput => "Validating output file...",
            Self::Completed => "Completed",
            Self::FailedRetryable => "Temporarily Failed (Retryable)",
            Self::FailedPermanent => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    pub current_stage: PipelineStage,
    pub total_segments: usize,
    pub completed_segments: usize,
    pub message: String,
}

impl Default for JobProgress {
    fn default() -> Self {
        Self {
            current_stage: PipelineStage::Idle,
            total_segments: 0,
            completed_segments: 0,
            message: "Ready".to_string(),
        }
    }
}

impl JobProgress {
    pub fn fraction(&self) -> f64 {
        match self.current_stage {
            PipelineStage::Idle => 0.0,
            PipelineStage::Validating => 0.05,
            PipelineStage::Uploading => 0.15,
            PipelineStage::Transcribing => 0.30,
            PipelineStage::Translating => 0.45,
            PipelineStage::Synthesizing => {
                if self.total_segments > 0 {
                    0.45 + 0.35 * (self.completed_segments as f64 / self.total_segments as f64)
                } else {
                    0.50
                }
            }
            PipelineStage::Aligning => 0.85,
            PipelineStage::Exporting => 0.92,
            PipelineStage::ValidatingOutput => 0.98,
            PipelineStage::Completed => 1.0,
            PipelineStage::FailedRetryable
            | PipelineStage::FailedPermanent
            | PipelineStage::Cancelled => 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub source_audio: AudioDocument,
    pub source_language: LanguageId,
    pub target_language: LanguageId,
    pub stage: PipelineStage,
    pub progress: JobProgress,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub error_message: Option<String>,
}

impl Job {
    pub fn new(
        source_audio: AudioDocument,
        source_language: LanguageId,
        target_language: LanguageId,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: JobId::generate(),
            source_audio,
            source_language,
            target_language,
            stage: PipelineStage::Idle,
            progress: JobProgress::default(),
            created_at: now,
            updated_at: now,
            error_message: None,
        }
    }

    pub fn transition_to(&mut self, next: PipelineStage) -> Result<(), String> {
        // Idempotent re-entry: re-announcing the current stage (e.g. resume
        // paths or sub-steps like video audio-extraction that reuses the
        // Validating stage) must not fail the whole job.
        if self.stage == next {
            self.progress.current_stage = next;
            self.updated_at = Utc::now();
            return Ok(());
        }
        if !self.stage.can_transition_to(&next) {
            return Err(format!(
                "Invalid state transition from {:?} to {:?}",
                self.stage, next
            ));
        }
        self.stage = next;
        self.progress.current_stage = next;
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn fail(&mut self, retryable: bool, message: String) {
        let next = if retryable {
            PipelineStage::FailedRetryable
        } else {
            PipelineStage::FailedPermanent
        };
        self.stage = next;
        self.progress.current_stage = next;
        self.error_message = Some(message);
        self.updated_at = Utc::now();
    }

    pub fn cancel(&mut self) {
        self.stage = PipelineStage::Cancelled;
        self.progress.current_stage = PipelineStage::Cancelled;
        self.progress.message = "Job cancelled by user".to_string();
        self.updated_at = Utc::now();
    }
}
