use tokio::sync::watch;

use crate::{
    audio,
    inject,
    mode,
    speech::{openai_compatible::OpenAiCompatibleProvider, SpeechProvider},
    state::{AppState, RecordingState, SessionController},
};

/// テキスト注入の抽象化。テストでモック可能にするために定義する。
pub trait TextInjector: Send + Sync {
    fn inject(&self, text: &str) -> anyhow::Result<()>;
}

/// 実際のクリップボード注入実装。
pub struct ClipboardInjector;

impl TextInjector for ClipboardInjector {
    fn inject(&self, text: &str) -> anyhow::Result<()> {
        inject::inject_text_after_f4(text)
    }
}

/// 録音開始の本体。AppHandle 不要のため単体テスト可能。
pub async fn start_session_inner(state: &AppState) -> Result<(), String> {
    let mut session = state.session.lock().await;
    if session.is_some() {
        return Ok(());
    }

    let (stop_tx, stop_rx) = watch::channel(false);
    let device_id = state.settings.lock().await.device_id.clone();
    let input = audio::new_input(device_id.as_deref());
    let capture_task =
        tokio::task::spawn_blocking(move || input.capture_blocking(stop_rx));

    *session = Some(SessionController { stop_tx, capture_task });
    *state.recording_state.lock().await = RecordingState::Recording;
    Ok(())
}

/// 録音停止 → ASR → 注入の本体。AppHandle 不要のため単体テスト可能。
///
/// 成否にかかわらず `RecordingState::Idle` に戻すことを保証する。
pub async fn stop_session_inner(
    state: &AppState,
    injector: &dyn TextInjector,
) -> Result<String, String> {
    let controller = state.session.lock().await.take();
    let Some(controller) = controller else {
        return Ok(String::new());
    };

    *state.recording_state.lock().await = RecordingState::Processing;

    let result: Result<String, String> = async {
        let _ = controller.stop_tx.send(true);

        let audio = controller
            .capture_task
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

        let current_settings = state.settings.lock().await.clone();
        if current_settings.api_key.is_empty() {
            return Err("APIキーが設定されていません。設定画面から入力してください。".to_string());
        }

        let raw_text = OpenAiCompatibleProvider {
            base_url: current_settings.api_base_url,
            api_key: current_settings.api_key,
            model: current_settings.api_model,
        }
        .transcribe(&audio)
        .await
        .map_err(|e| e.to_string())?;

        let current_mode = state.mode.lock().await.clone();
        let current_settings_for_mode = state.settings.lock().await.clone();
        let final_text = mode::route(&current_mode, &current_settings_for_mode, &raw_text).await;

        injector.inject(&final_text).map_err(|e| e.to_string())?;

        Ok(final_text)
    }
    .await;

    // 成否にかかわらず必ず Idle に戻す
    *state.recording_state.lock().await = RecordingState::Idle;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::CapturedAudio;
    use crate::state::AppState;

    struct NoOpInjector;
    impl TextInjector for NoOpInjector {
        fn inject(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[allow(dead_code)]
    struct FailingInjector;
    impl TextInjector for FailingInjector {
        fn inject(&self, _: &str) -> anyhow::Result<()> {
            anyhow::bail!("injection failed")
        }
    }

    /// セッション付きの AppState を作る。キャプチャタスクは即座に空音声を返す。
    async fn make_state_with_session() -> AppState {
        let state = AppState::default();
        let (stop_tx, stop_rx) = watch::channel(false);
        let capture_task = tokio::task::spawn_blocking(move || {
            drop(stop_rx);
            Ok::<CapturedAudio, anyhow::Error>(CapturedAudio::default())
        });
        *state.session.lock().await =
            Some(SessionController { stop_tx, capture_task });
        state
    }

    #[tokio::test]
    async fn stop_without_session_returns_empty() {
        let state = AppState::default();
        let result = stop_session_inner(&state, &NoOpInjector).await;
        assert_eq!(result.unwrap(), "");
    }

    #[tokio::test]
    async fn stop_with_empty_api_key_returns_err() {
        let state = make_state_with_session().await;
        // api_key はデフォルトで空
        let result = stop_session_inner(&state, &NoOpInjector).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("APIキー"));
    }

    #[tokio::test]
    async fn stop_always_resets_state_to_idle() {
        let state = make_state_with_session().await;
        // api_key 空でエラーになっても Idle に戻ること
        let result = stop_session_inner(&state, &NoOpInjector).await;
        assert!(result.is_err());
        assert!(matches!(
            *state.recording_state.lock().await,
            RecordingState::Idle
        ));
    }

    #[tokio::test]
    async fn start_session_sets_recording_state() {
        let state = AppState::default();
        start_session_inner(&state).await.unwrap();
        assert!(matches!(
            *state.recording_state.lock().await,
            RecordingState::Recording
        ));
        // WASAPI タスクをアボートして後続のテストがハングしないようにする
        if let Some(controller) = state.session.lock().await.take() {
            controller.capture_task.abort();
        };
    }

    #[tokio::test]
    async fn start_session_is_idempotent() {
        let state = AppState::default();
        start_session_inner(&state).await.unwrap();
        start_session_inner(&state).await.unwrap();
        // セッションが1つだけであることを確認
        assert!(state.session.lock().await.is_some());
        // WASAPI タスクをアボートして後続のテストがハングしないようにする
        if let Some(controller) = state.session.lock().await.take() {
            controller.capture_task.abort();
        };
    }
}
