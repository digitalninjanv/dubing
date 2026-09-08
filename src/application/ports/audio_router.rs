use crate::domain::{AudioAppInfo, DomainError};
use async_trait::async_trait;

#[async_trait]
pub trait AudioRouter: Send + Sync {
    /// Checks if the audio routing system (e.g. pactl / PipeWire / PulseAudio) is available.
    async fn is_available(&self) -> bool;

    /// Creates a virtual null-sink (e.g. "AudioDub_Virtual_Sink") to silence original audio from speakers.
    /// Returns the module ID created by the audio server.
    async fn create_null_sink(&self, sink_name: &str) -> Result<u32, DomainError>;

    /// Unloads the virtual null-sink by its module ID.
    async fn unload_null_sink(&self, module_id: u32) -> Result<(), DomainError>;

    /// Lists currently active playback applications (e.g. Chrome, Firefox, Spotify).
    async fn list_sink_inputs(&self) -> Result<Vec<AudioAppInfo>, DomainError>;

    /// Moves an application's playback to `sink_name` and returns the sink it was using before
    /// the move.  Callers must preserve this value so stopping a live session does not change a
    /// user's audio-device selection.
    async fn move_sink_input(
        &self,
        sink_input_id: u32,
        sink_name: &str,
    ) -> Result<String, DomainError>;

    /// Restores an application's audio playback to its original sink.
    async fn restore_sink_input(
        &self,
        sink_input_id: u32,
        original_sink: &str,
    ) -> Result<(), DomainError>;

    /// Gets the name of the default physical audio output sink.
    async fn get_default_sink_name(&self) -> Result<String, DomainError>;

    /// Returns the monitor source belonging to the current default output sink.  This is the
    /// correct source for desktop capture; `default` commonly means the microphone.
    async fn get_default_monitor_source(&self) -> Result<String, DomainError>;

    /// Returns the current default input source for microphone capture.
    async fn get_default_source_name(&self) -> Result<String, DomainError>;

    /// Verifies that a PulseAudio/PipeWire-Pulse source exists before FFmpeg is started.
    async fn source_exists(&self, source_name: &str) -> Result<bool, DomainError>;
}
