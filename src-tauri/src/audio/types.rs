use std::time::Duration;
use tokio::sync::watch;

/// キャプチャした音声データ。samples は interleaved PCM (-1.0..1.0)。
#[derive(Debug, Clone, Default)]
pub struct CapturedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl CapturedAudio {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.frames() as f64 / self.sample_rate.max(1) as f64)
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// 音声入力デバイスの共通インターフェース。
/// ブロッキング関数として定義し、`spawn_blocking` から呼ぶ前提。
pub trait AudioInput: Send + Sync {
    fn capture_blocking(&self, stop_rx: watch::Receiver<bool>) -> anyhow::Result<CapturedAudio>;
}

/// Windows 以外のプラットフォーム向けスタブ。
pub struct UnsupportedAudioInput;

impl AudioInput for UnsupportedAudioInput {
    fn capture_blocking(&self, _stop_rx: watch::Receiver<bool>) -> anyhow::Result<CapturedAudio> {
        anyhow::bail!("audio capture is only supported on Windows")
    }
}
