pub mod mock;
pub mod openai_compatible;

/// 音声認識プロバイダの共通インターフェース。
/// 将来 OpenAI 互換 API 実装に差し替える前提で trait 化している。
#[async_trait::async_trait]
pub trait SpeechProvider: Send + Sync {
    async fn transcribe(&self, audio: &[f32]) -> anyhow::Result<String>;
}
