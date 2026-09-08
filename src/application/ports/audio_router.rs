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

    /// Moves an application's audio playback to the specified sink (e.g. "AudioDub_Virtual_Sink").
    async fn move_sink_input(&self, sink_input_id: u32, sink_name: &str) -> Result<(), DomainError>;

    /// Restores an application's audio playback to the default hardware sink.
    async fn restore_sink_input(&self, sink_input_id: u32) -> Result<(), DomainError>;

    /// Gets the name of the default physical audio output sink.
    async fn get_default_sink_name(&self) -> Result<String, DomainError>;
}
