use std::time::Instant;

use tokio::sync::watch;

use crate::{
    audio, context, inject, mode,
    speech::{openai_compatible::OpenAiCompatibleProvider, SpeechProvider},
    state::{AppState, Mode, RecordingState, SessionController},
};

/// テキスト注入の抽象化。テストでモック可能にするために定義する。
pub trait TextInjector: Send + Sync {
    fn inject(&self, text: &str) -> anyhow::Result<()>;
}

/// 実際のクリップボード注入実装。
pub struct ClipboardInjector;

impl TextInjector for ClipboardInjector {
    fn inject(&self, text: &str) -> anyhow::Result<()> {
        inject::inject_text(text)
    }
}

#[derive(Debug)]
pub struct SessionOutcome {
    pub raw_text: String,
    pub final_text: String,
    pub mode: Mode,
    pub duration_ms: u64,
}

/// 録音開始の本体。AppHandle 不要のため単体テスト可能。
pub async fn start_session_inner(
    state: &AppState,
    level_tx: Option<tokio::sync::mpsc::UnboundedSender<f32>>,
) -> Result<(), String> {
    let mut session = state.session.lock().await;
    if session.is_some() {
        return Ok(());
    }

    let (stop_tx, stop_rx) = watch::channel(false);
    let device_id = state.settings.lock().await.device_id.clone();
    let input = audio::new_input(device_id.as_deref(), level_tx);
    let capture_task = tokio::task::spawn_blocking(move || input.capture_blocking(stop_rx));

    *session = Some(SessionController {
        stop_tx,
        capture_task,
        started_at: Instant::now(),
    });
    *state.recording_state.lock().await = RecordingState::Recording;
    Ok(())
}

/// 録音停止 → ASR → 注入の本体。AppHandle 不要のため単体テスト可能。
///
/// 成否にかかわらず `RecordingState::Idle` に戻すことを保証する。
pub async fn stop_session_inner(
    state: &AppState,
    injector: &dyn TextInjector,
) -> Result<SessionOutcome, String> {
    let controller = state.session.lock().await.take();
    let Some(controller) = controller else {
        return Ok(SessionOutcome {
            raw_text: String::new(),
            final_text: String::new(),
            mode: state.mode.lock().await.clone(),
            duration_ms: 0,
        });
    };

    *state.recording_state.lock().await = RecordingState::Processing;

    let result: Result<SessionOutcome, String> = async {
        let _ = controller.stop_tx.send(true);

        let audio = controller
            .capture_task
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

        let duration_ms = controller.started_at.elapsed().as_millis() as u64;
        let current_settings = state.settings.lock().await.clone();
        if current_settings.api_key.is_empty() {
            return Err("APIキーが設定されていません。設定画面から入力してください。".to_string());
        }
        let dictionary_words = state.dictionary_words.lock().await.clone();
        let focused_context = if current_settings.deep_context_enabled {
            context::focused_app_context()
        } else {
            None
        };

        let raw_text = OpenAiCompatibleProvider {
            base_url: current_settings.api_base_url,
            api_key: current_settings.api_key,
            model: current_settings.api_model,
            dictionary_words,
            focused_context: focused_context.clone(),
        }
        .transcribe(&audio)
        .await
        .map_err(|e| e.to_string())?;

        let current_mode = state.mode.lock().await.clone();
        let current_settings_for_mode = state.settings.lock().await.clone();
        let current_dictionary_words = state.dictionary_words.lock().await.clone();
        let final_text = mode::route(
            &current_mode,
            &current_settings_for_mode,
            &current_dictionary_words,
            focused_context.as_ref(),
            &raw_text,
        )
        .await;

        injector.inject(&final_text).map_err(|e| e.to_string())?;

        Ok(SessionOutcome {
            raw_text,
            final_text,
            mode: current_mode,
            duration_ms,
        })
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
        *state.session.lock().await = Some(SessionController {
            stop_tx,
            capture_task,
            started_at: Instant::now(),
        });
        state
    }

    #[tokio::test]
    async fn stop_without_session_returns_empty() {
        let state = AppState::default();
        let result = stop_session_inner(&state, &NoOpInjector).await;
        assert_eq!(result.unwrap().final_text, "");
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
    async fn start_session_is_idempotent_when_session_exists() {
        let state = AppState::default();
        let existing = make_state_with_session().await;
        let controller = existing.session.lock().await.take().unwrap();
        *state.session.lock().await = Some(controller);

        start_session_inner(&state, None).await.unwrap();
        assert!(state.session.lock().await.is_some());
        if let Some(controller) = state.session.lock().await.take() {
            controller.capture_task.abort();
        };
    }
}
