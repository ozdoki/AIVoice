use super::SpeechProvider;
use crate::audio::CapturedAudio;

/// モック実装。マイクや API なしで固定文字列を返す。
/// 「モック縦切り」検証用。
pub struct MockSpeechProvider {
    pub fixed_text: String,
}

impl Default for MockSpeechProvider {
    fn default() -> Self {
        Self {
            fixed_text: "こんにちは、KoeType のテスト入力です。".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl SpeechProvider for MockSpeechProvider {
    async fn transcribe(&self, _audio: &CapturedAudio) -> anyhow::Result<String> {
        Ok(self.fixed_text.clone())
    }
}
