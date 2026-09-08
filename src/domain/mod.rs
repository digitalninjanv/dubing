pub mod audio;
pub mod batch;
pub mod errors;
pub mod job;
pub mod language;
pub mod live_dubber;
pub mod subtitle;
pub mod synthesis;
pub mod transcript;
pub mod translation;

pub use audio::{AudioDocument, AudioFormat, MediaMetadata};
pub use batch::{BatchItem, BatchItemStatus, BatchJob};
pub use errors::DomainError;
pub use job::{DubbingEngine, Job, JobId, JobProgress, PipelineStage};
pub use language::{LanguageId, LanguageInfo, LanguageRegistry};
pub use live_dubber::{
    AudioAppInfo, AudioSourceMode, LiveDubberStatus, LiveModelChoice, LiveTranscriptUpdate,
};
pub use subtitle::{
    format_timestamp_srt, format_timestamp_vtt, generate_bilingual_txt, generate_srt, generate_vtt,
    SubtitleFormat,
};
pub use synthesis::{
    AlignmentResult, AudioArtifact, SpeakerVoiceConfig, SynthesizedSegment, TtsRequest,
    TtsStylePreset, VoiceProfile,
};
pub use transcript::{Transcript, TranscriptSegment, WordTimestamp};
pub use translation::{TranslatedDocument, TranslationSegment, TranslationTone};
