pub mod types;
#[cfg(target_os = "windows")]
pub mod wasapi;

pub use types::{AudioDeviceInfo, AudioInput, CapturedAudio};

/// 指定したデバイス ID（省略時はデフォルトデバイス）で録音入力を生成する。
pub fn new_input(device_id: Option<&str>) -> std::sync::Arc<dyn AudioInput> {
    #[cfg(target_os = "windows")]
    return std::sync::Arc::new(wasapi::WasapiInput {
        device_id: device_id.map(|s| s.to_string()),
    });
    #[cfg(not(target_os = "windows"))]
    {
        let _ = device_id;
        return std::sync::Arc::new(types::UnsupportedAudioInput);
    }
}
