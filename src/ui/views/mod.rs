pub mod dropzone;
pub mod history;
pub mod live_dubber;
pub mod progress;
pub mod result;
pub mod review;
pub mod settings;
pub mod tts_studio;

pub use dropzone::DropzoneView;
pub use history::HistoryView;
pub use live_dubber::LiveDubberView;
pub use progress::ProgressView;
pub use result::ResultView;
pub use review::{ReviewResponder, ReviewTranscriptView};
pub use settings::SettingsDialog;
pub use tts_studio::TtsStudioView;
