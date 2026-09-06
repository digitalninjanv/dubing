pub mod audio;
pub mod errors;
pub mod job;
pub mod language;
pub mod synthesis;
pub mod transcript;
pub mod translation;

pub use audio::{AudioDocument, AudioFormat, MediaMetadata};
pub use errors::DomainError;
pub use job::{Job, JobId, JobProgress, PipelineStage};
pub use language::{LanguageId, LanguageInfo, LanguageRegistry};
pub use synthesis::{AlignmentResult, AudioArtifact, SynthesizedSegment, VoiceProfile};
pub use transcript::{Transcript, TranscriptSegment, WordTimestamp};
pub use translation::{TranslatedDocument, TranslationSegment};
