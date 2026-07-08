use std::time::{Duration, Instant};

use tauri::Emitter;
use tokio::sync::watch;

use crate::{
    audio, context, inject, local_data, mode, recovery,
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
    pub inject_error: Option<String>,
    pub recovery_id: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct PhaseEvent {
    phase: String,
}

fn emit_phase(app: Option<&tauri::AppHandle>, phase: &str) {
    if let Some(app) = app {
        let _ = app.emit(
            "session://phase-changed",
            PhaseEvent {
                phase: phase.to_string(),
            },
        );
    }
}

/// 録音開始の本体。AppHandle 不要のため単体テスト可能。
pub async fn start_session_inner(
    state: &AppState,
    level_tx: Option<tokio::sync::mpsc::UnboundedSender<f32>>,
    chunk_tx: Option<tokio::sync::mpsc::UnboundedSender<audio::AudioChunk>>,
    realtime_task: Option<tokio::task::JoinHandle<Result<String, String>>>,
    recovery_id: Option<String>,
    recovery_audio_path: Option<std::path::PathBuf>,
    mode: Mode,
    polish_preset: String,
) -> Result<(), String> {
    let mut session = state.session.lock().await;
    if session.is_some() {
        return Ok(());
    }

    let (stop_tx, stop_rx) = watch::channel(false);
    let device_id = state.settings.lock().await.device_id.clone();
    let input = audio::new_input(
        device_id.as_deref(),
        level_tx,
        chunk_tx,
        recovery_audio_path,
    );
    let capture_task = tokio::task::spawn_blocking(move || input.capture_blocking(stop_rx));

    *session = Some(SessionController {
        stop_tx,
        capture_task,
        realtime_task,
        started_at: Instant::now(),
        recovery_id,
        mode,
        polish_preset,
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
    app: Option<tauri::AppHandle>,
) -> Result<SessionOutcome, String> {
    let controller = state.session.lock().await.take();
    let Some(controller) = controller else {
        return Ok(SessionOutcome {
            raw_text: String::new(),
            final_text: String::new(),
            mode: state.mode.lock().await.clone(),
            duration_ms: 0,
            inject_error: None,
            recovery_id: None,
        });
    };

    *state.recording_state.lock().await = RecordingState::Processing;
    let recovery_id = controller.recovery_id.clone();
    let session_mode = controller.mode.clone();
    let session_polish_preset = controller.polish_preset.clone();

    let result: Result<SessionOutcome, String> = async {
        let stop_started = Instant::now();
        let _ = controller.stop_tx.send(true);

        let audio = controller
            .capture_task
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let capture_stop_ms = stop_started.elapsed().as_millis() as u64;

        let duration_ms = controller.started_at.elapsed().as_millis() as u64;
        if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
            recovery::mark_captured(
                app,
                id,
                audio.sample_rate,
                audio.channels,
                audio.frames(),
                duration_ms,
            )
            .map_err(|e| e.to_string())?;
        }
        let current_settings = state.settings.lock().await.clone();
        if current_settings.api_key.is_empty() {
            if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
                let _ = recovery::mark_failed(
                    app,
                    id,
                    "APIキーが設定されていません。設定画面から入力してください。".to_string(),
                );
            }
            return Err("APIキーが設定されていません。設定画面から入力してください。".to_string());
        }
        let dictionary_words = state.dictionary_words.lock().await.clone();
        let focused_context = if current_settings.deep_context_enabled {
            context::focused_app_context()
        } else {
            None
        };
        emit_phase(app.as_ref(), "transcribing");
        if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
            recovery::mark_transcribing(app, id).map_err(|e| e.to_string())?;
        }

        let realtime_wait_started = Instant::now();
        let realtime_text = match controller.realtime_task {
            Some(task) => match tokio::time::timeout(Duration::from_secs(8), task).await {
                Ok(Ok(Ok(text))) if !text.trim().is_empty() => Some(text),
                Ok(Ok(Ok(_))) => {
                    tracing::warn!("Realtime ASR returned empty text; falling back to batch ASR");
                    None
                }
                Ok(Ok(Err(error))) => {
                    tracing::warn!("Realtime ASR failed; falling back to batch ASR: {error}");
                    None
                }
                Ok(Err(error)) => {
                    tracing::warn!(
                        "Realtime ASR task join failed; falling back to batch ASR: {error}"
                    );
                    None
                }
                Err(_) => {
                    tracing::warn!("Realtime ASR final timed out; falling back to batch ASR");
                    None
                }
            },
            None => None,
        };
        let realtime_wait_ms = realtime_wait_started.elapsed().as_millis() as u64;

        let provider = OpenAiCompatibleProvider {
            base_url: current_settings.api_base_url,
            api_key: current_settings.api_key,
            model: current_settings.api_model,
            dictionary_words,
            focused_context: focused_context.clone(),
            partial_tx: None,
        };
        let mut batch_asr_ms = 0_u64;
        let raw_text = match realtime_text {
            Some(text) => text,
            None => {
                let batch_started = Instant::now();
                match provider.transcribe(&audio).await {
                    Ok(text) => {
                        batch_asr_ms = batch_started.elapsed().as_millis() as u64;
                        text
                    }
                    Err(error) => {
                        batch_asr_ms = batch_started.elapsed().as_millis() as u64;
                        drop(provider);
                        let error = error.to_string();
                        if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
                            let _ = recovery::mark_failed(app, id, error.clone());
                        }
                        tracing::info!(
                            capture_stop_ms,
                            realtime_wait_ms,
                            batch_asr_ms,
                            total_after_stop_ms = stop_started.elapsed().as_millis() as u64,
                            mode = ?session_mode,
                            "session failed during ASR"
                        );
                        return Err(error);
                    }
                }
            }
        };
        drop(provider);

        let current_mode = session_mode.clone();
        let mut current_settings_for_mode = state.settings.lock().await.clone();
        current_settings_for_mode.polish_preset = session_polish_preset;
        let current_dictionary_words = state.dictionary_words.lock().await.clone();
        if matches!(current_mode, Mode::Polish) {
            emit_phase(app.as_ref(), "polishing");
        }
        let final_text = mode::route(
            &current_mode,
            &current_settings_for_mode,
            &current_dictionary_words,
            focused_context.as_ref(),
            &raw_text,
        )
        .await;
        let final_text = if let Some(app) = app.as_ref() {
            let snippets = local_data::load_snippets(app).map_err(|error| error.to_string())?;
            local_data::expand_snippets(&final_text, &snippets)
        } else {
            final_text
        };

        if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
            recovery::mark_text_ready(app, id, raw_text.clone(), final_text.clone())
                .map_err(|e| e.to_string())?;
        }

        emit_phase(app.as_ref(), "injecting");
        let inject_started = Instant::now();
        let inject_error = match injector.inject(&final_text) {
            Ok(()) => None,
            Err(error) => {
                let error = error.to_string();
                if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
                    let _ = recovery::mark_failed(app, id, error.clone());
                }
                Some(error)
            }
        };
        let inject_ms = inject_started.elapsed().as_millis() as u64;
        tracing::info!(
            capture_stop_ms,
            realtime_wait_ms,
            batch_asr_ms,
            inject_ms,
            total_after_stop_ms = stop_started.elapsed().as_millis() as u64,
            mode = ?current_mode,
            "session processing timings"
        );

        Ok(SessionOutcome {
            raw_text,
            final_text,
            mode: current_mode,
            duration_ms,
            inject_error,
            recovery_id,
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
            realtime_task: None,
            started_at: Instant::now(),
            recovery_id: None,
            mode: Mode::Raw,
            polish_preset: "memo".to_string(),
        });
        state
    }

    #[tokio::test]
    async fn stop_without_session_returns_empty() {
        let state = AppState::default();
        let result = stop_session_inner(&state, &NoOpInjector, None).await;
        assert_eq!(result.unwrap().final_text, "");
    }

    #[tokio::test]
    async fn stop_with_empty_api_key_returns_err() {
        let state = make_state_with_session().await;
        // api_key はデフォルトで空
        let result = stop_session_inner(&state, &NoOpInjector, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("APIキー"));
    }

    #[tokio::test]
    async fn stop_always_resets_state_to_idle() {
        let state = make_state_with_session().await;
        // api_key 空でエラーになっても Idle に戻ること
        let result = stop_session_inner(&state, &NoOpInjector, None).await;
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

        start_session_inner(
            &state,
            None,
            None,
            None,
            None,
            None,
            Mode::Raw,
            "memo".to_string(),
        )
        .await
        .unwrap();
        assert!(state.session.lock().await.is_some());
        if let Some(controller) = state.session.lock().await.take() {
            controller.capture_task.abort();
        };
    }
}
