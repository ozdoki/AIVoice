pub mod types;
#[cfg(target_os = "windows")]
pub mod wasapi;

pub use types::{AudioDeviceInfo, AudioInput, CapturedAudio};

/// 指定したデバイス ID（省略時はデフォルトデバイス）で録音入力を生成する。
/// level_tx を渡すと録音ループから RMS レベル（0.0–1.0）が送信される。
pub fn new_input(
    device_id: Option<&str>,
    level_tx: Option<tokio::sync::mpsc::UnboundedSender<f32>>,
) -> std::sync::Arc<dyn AudioInput> {
    #[cfg(target_os = "windows")]
    return std::sync::Arc::new(wasapi::WasapiInput {
        device_id: device_id.map(|s| s.to_string()),
        level_tx,
    });
    #[cfg(not(target_os = "windows"))]
    {
        let _ = device_id;
        let _ = level_tx;
        return std::sync::Arc::new(types::UnsupportedAudioInput);
    }
}
