pub mod types;
#[cfg(target_os = "windows")]
pub mod wasapi;

pub use types::{AudioInput, CapturedAudio};

#[cfg(target_os = "windows")]
pub fn default_input() -> std::sync::Arc<dyn AudioInput> {
    std::sync::Arc::new(wasapi::WasapiInput)
}

#[cfg(not(target_os = "windows"))]
pub fn default_input() -> std::sync::Arc<dyn AudioInput> {
    std::sync::Arc::new(types::UnsupportedAudioInput)
}
