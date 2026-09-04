use std::time::{Duration, Instant};

use tauri::Emitter;
use tokio::sync::watch;

use crate::{
    audio, context, corrections, inject, local_data, mode,
    polish::PolishState,
    recovery,
    settings::LanguageMode,
    speech::openai_compatible::OpenAiCompatibleProvider,
    state::{AppState, Mode, RecordingState, SessionController},
};

/// テキスト注入の抽象化。テストでモック可能にするために定義する。
pub trait TextInjector: Send + Sync {
    fn inject(&self, text: &str) -> anyhow::Result<inject::InjectionSuccess>;
}

/// 実際のクリップボード注入実装。
pub struct ClipboardInjector;

impl TextInjector for ClipboardInjector {
    fn inject(&self, text: &str) -> anyhow::Result<inject::InjectionSuccess> {
        inject::inject_text(text)
    }
}

#[derive(Debug)]
pub struct SessionOutcome {
    pub raw_text: String,
    pub final_text: String,
    pub mode: Mode,
    pub polish_state: PolishState,
    pub duration_ms: u64,
    pub inject_error: Option<String>,
    pub inject_warning: Option<String>,
    pub recovery_id: Option<String>,
    pub polish_preset: String,
    pub app_process: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CancelSessionOutcome {
    pub cancelled: bool,
    pub recovery_id: Option<String>,
    pub capture_error: Option<String>,
}

fn prepare_post_asr_pipeline(
    snapshot: &corrections::CorrectionSessionSnapshot,
    raw_text: &str,
    polish_preset: &str,
    app_process: &str,
) -> (String, Vec<String>, Vec<corrections::StyleExample>) {
    corrections::apply_session_snapshot(snapshot, raw_text, polish_preset, app_process)
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
    app_process: String,
    correction_snapshot: corrections::CorrectionSessionSnapshot,
    language_mode: LanguageMode,
    target_window: Option<crate::context::FocusedWindowTarget>,
) -> Result<(), String> {
    let mut session = state.session.lock().await;
    if session.is_some() {
        return Ok(());
    }
    state.reset_cancellation_gate();

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
        app_process,
        correction_snapshot,
        language_mode,
        target_window,
    });
    *state.recording_state.lock().await = RecordingState::Recording;
    Ok(())
}

/// 録音中のセッションを処理へ進めず破棄する。
///
/// 音声キャプチャには停止を通知して終了を待つが、Realtime ASRは直ちにabortする。
/// ASR、Polish、注入、履歴、利用量の処理はこの経路から呼ばれない。
pub async fn cancel_session_inner(state: &AppState) -> CancelSessionOutcome {
    if !matches!(
        *state.recording_state.lock().await,
        RecordingState::Recording
    ) {
        return CancelSessionOutcome {
            cancelled: false,
            recovery_id: None,
            capture_error: None,
        };
    }

    let controller = state.session.lock().await.take();
    let Some(controller) = controller else {
        *state.recording_trigger.lock().await = None;
        *state.recording_state.lock().await = RecordingState::Idle;
        state.reset_cancellation_gate();
        return CancelSessionOutcome {
            cancelled: false,
            recovery_id: None,
            capture_error: Some("録音セッションが見つかりませんでした。".to_string()),
        };
    };

    let recovery_id = controller.recovery_id.clone();
    let _ = controller.stop_tx.send(true);
    if let Some(task) = controller.realtime_task {
        task.abort();
        // outer futureをawaitし、内部のabort-on-drop sender guardも確実にdropさせる。
        let _ = task.await;
    }
    let capture_error = match controller.capture_task.await {
        Ok(Ok(_)) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(error) => Some(error.to_string()),
    };

    *state.recording_trigger.lock().await = None;
    *state.recording_state.lock().await = RecordingState::Idle;
    state.reset_cancellation_gate();
    CancelSessionOutcome {
        cancelled: true,
        recovery_id,
        capture_error,
    }
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
        state.reset_cancellation_gate();
        return Ok(SessionOutcome {
            raw_text: String::new(),
            final_text: String::new(),
            mode: state.mode.lock().await.clone(),
            polish_state: PolishState::Unknown,
            duration_ms: 0,
            inject_error: None,
            inject_warning: None,
            recovery_id: None,
            polish_preset: String::new(),
            app_process: String::new(),
        });
    };

    *state.recording_state.lock().await = RecordingState::Processing;
    let recovery_id = controller.recovery_id.clone();
    let session_mode = controller.mode.clone();
    let session_polish_preset = controller.polish_preset.clone();
    let session_app_process = controller.app_process.clone();
    let correction_snapshot = controller.correction_snapshot.clone();
    let session_language_mode = controller.language_mode;

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
        let dictionary_words = correction_snapshot.dictionary_words.clone();
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
            language_mode: session_language_mode,
            dictionary_words,
            focused_context: focused_context.clone(),
            partial_tx: None,
        };
        let mut batch_asr_ms = 0_u64;
        let raw_text = match realtime_text {
            Some(text) => text,
            None => {
                let batch_started = Instant::now();
                match provider
                    .transcribe_dictation(&audio, app.as_ref(), recovery_id.as_deref())
                    .await
                {
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

        if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
            recovery::save_raw_text(app, id, raw_text.clone()).map_err(|e| e.to_string())?;
        }

        let current_mode = session_mode.clone();
        let mut current_settings_for_mode = state.settings.lock().await.clone();
        current_settings_for_mode.polish_preset = session_polish_preset.clone();
        let (post_asr_text, current_dictionary_words, style_examples) = prepare_post_asr_pipeline(
            &correction_snapshot,
            &raw_text,
            &session_polish_preset,
            &session_app_process,
        );
        if matches!(current_mode, Mode::Polish) {
            emit_phase(app.as_ref(), "polishing");
        }
        let routed = mode::route_with_style_examples(
            &current_mode,
            &current_settings_for_mode,
            &current_dictionary_words,
            focused_context.as_ref(),
            &style_examples,
            &post_asr_text,
        )
        .await;
        let polish_state = routed.polish_state;
        let final_text = if let Some(app) = app.as_ref() {
            let snippets = local_data::load_snippets(app).map_err(|error| error.to_string())?;
            local_data::expand_snippets(&routed.text, &snippets)
        } else {
            routed.text
        };

        if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
            recovery::mark_text_ready(app, id, raw_text.clone(), final_text.clone())
                .map_err(|e| e.to_string())?;
        }

        emit_phase(app.as_ref(), "injecting");
        let inject_started = Instant::now();
        let (inject_error, inject_warning) = match injector.inject(&final_text) {
            Ok(success) => (None, success.warning),
            Err(error) => {
                let error = error.to_string();
                if let (Some(app), Some(id)) = (app.as_ref(), recovery_id.as_ref()) {
                    let _ = recovery::mark_failed(app, id, error.clone());
                }
                (Some(error), None)
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
            polish_state,
            duration_ms,
            inject_error,
            inject_warning,
            recovery_id,
            polish_preset: session_polish_preset,
            app_process: session_app_process,
        })
    }
    .await;

    // 成否にかかわらず必ず Idle に戻す
    *state.recording_state.lock().await = RecordingState::Idle;
    state.reset_cancellation_gate();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::CapturedAudio;
    use crate::state::AppState;

    struct NoOpInjector;
    impl TextInjector for NoOpInjector {
        fn inject(&self, _: &str) -> anyhow::Result<inject::InjectionSuccess> {
            Ok(inject::InjectionSuccess { warning: None })
        }
    }

    struct FailingInjector;
    impl TextInjector for FailingInjector {
        fn inject(&self, _: &str) -> anyhow::Result<inject::InjectionSuccess> {
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
            app_process: "notepad.exe".to_string(),
            correction_snapshot: corrections::CorrectionSessionSnapshot::default(),
            language_mode: LanguageMode::Auto,
            target_window: None,
        });
        *state.recording_state.lock().await = RecordingState::Recording;
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
            "notepad.exe".to_string(),
            corrections::CorrectionSessionSnapshot::default(),
            LanguageMode::Auto,
            None,
        )
        .await
        .unwrap();
        assert!(state.session.lock().await.is_some());
        if let Some(controller) = state.session.lock().await.take() {
            controller.capture_task.abort();
        };
    }

    #[tokio::test]
    async fn cancel_without_session_is_a_noop() {
        let state = AppState::default();
        let outcome = cancel_session_inner(&state).await;
        assert!(!outcome.cancelled);
        assert!(outcome.recovery_id.is_none());
        assert!(outcome.capture_error.is_none());
        assert!(matches!(
            *state.recording_state.lock().await,
            RecordingState::Idle
        ));
    }

    #[test]
    fn injector_failure_is_reported_instead_of_being_treated_as_success() {
        let error = FailingInjector.inject("本文").unwrap_err().to_string();
        assert!(error.contains("injection failed"));
    }

    #[test]
    fn stop_pipeline_preparation_uses_active_snapshot_artifacts_for_anonymous_corpus() {
        use crate::{
            corrections::{
                CorrectionArtifact, CorrectionClassification, CorrectionStore, LearningScope,
                NewCorrection,
            },
            settings::CorrectionLearningMode,
        };

        let mut store = CorrectionStore::default();
        corrections::insert(
            &mut store,
            NewCorrection {
                source_history_id: "history-anonymous-1".to_string(),
                raw_text: "Project Atlasをopenする".to_string(),
                original_text: "Project Atlasをopenする".to_string(),
                corrected_text: "ProjectAtlasを開く".to_string(),
                mode: Mode::Polish,
                polish_preset: "memo".to_string(),
                app_process: "editor.exe".to_string(),
                classification: CorrectionClassification::Minor,
                artifacts: vec![
                    CorrectionArtifact::Replacement {
                        from: "Project Atlas".to_string(),
                        to: "ProjectAtlas".to_string(),
                        scope: LearningScope::Global,
                    },
                    CorrectionArtifact::Vocabulary {
                        value: "ProjectAtlas".to_string(),
                        scope: LearningScope::Global,
                    },
                    CorrectionArtifact::StyleExample {
                        input: "openする".to_string(),
                        output: "開く".to_string(),
                    },
                ],
            },
        )
        .unwrap();
        let snapshot = corrections::make_session_snapshot(
            CorrectionLearningMode::Ask,
            &["ManualTerm".to_string()],
            store,
            "editor.exe",
        );

        let (post_asr, dictionary, examples) =
            prepare_post_asr_pipeline(&snapshot, "Project Atlasをopenする", "memo", "editor.exe");

        assert_eq!(post_asr, "ProjectAtlasをopenする");
        assert_eq!(dictionary, vec!["ManualTerm", "ProjectAtlas"]);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].input, "openする");
        assert_eq!(examples[0].output, "開く");
    }

    #[tokio::test]
    async fn cancel_discards_each_recording_mode_and_clears_trigger() {
        for trigger in [
            crate::state::RecordingTrigger::PushToTalk,
            crate::state::RecordingTrigger::HandsFree,
            crate::state::RecordingTrigger::Manual,
        ] {
            for mode in [Mode::Raw, Mode::Polish] {
                let state = make_state_with_session().await;
                state.session.lock().await.as_mut().unwrap().mode = mode;
                *state.recording_trigger.lock().await = Some(trigger.clone());

                let outcome = cancel_session_inner(&state).await;

                assert!(outcome.cancelled);
                assert!(outcome.capture_error.is_none());
                assert!(state.session.lock().await.is_none());
                assert!(state.recording_trigger.lock().await.is_none());
                assert!(matches!(
                    *state.recording_state.lock().await,
                    RecordingState::Idle
                ));
            }
        }
    }

    #[tokio::test]
    async fn cancel_aborts_realtime_task() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };

        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let state = make_state_with_session().await;
        let dropped = Arc::new(AtomicBool::new(false));
        let task_dropped = dropped.clone();
        let realtime_task = tokio::spawn(async move {
            let _flag = DropFlag(task_dropped);
            std::future::pending::<()>().await;
            Ok::<String, String>(String::new())
        });
        tokio::task::yield_now().await;
        state.session.lock().await.as_mut().unwrap().realtime_task = Some(realtime_task);

        let outcome = cancel_session_inner(&state).await;
        tokio::task::yield_now().await;

        assert!(outcome.cancelled);
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn cancel_returns_idle_even_when_capture_fails() {
        let state = AppState::default();
        let (stop_tx, stop_rx) = watch::channel(false);
        let capture_task = tokio::task::spawn_blocking(move || {
            drop(stop_rx);
            Err::<CapturedAudio, anyhow::Error>(anyhow::anyhow!("capture failed"))
        });
        *state.session.lock().await = Some(SessionController {
            stop_tx,
            capture_task,
            realtime_task: None,
            started_at: Instant::now(),
            recovery_id: Some("recovery-test".to_string()),
            mode: Mode::Raw,
            polish_preset: "memo".to_string(),
            app_process: "notepad.exe".to_string(),
            correction_snapshot: corrections::CorrectionSessionSnapshot::default(),
            language_mode: LanguageMode::Auto,
            target_window: None,
        });
        *state.recording_state.lock().await = RecordingState::Recording;

        let outcome = cancel_session_inner(&state).await;

        assert!(outcome.cancelled);
        assert_eq!(outcome.recovery_id.as_deref(), Some("recovery-test"));
        assert!(outcome.capture_error.unwrap().contains("capture failed"));
        assert!(matches!(
            *state.recording_state.lock().await,
            RecordingState::Idle
        ));
    }

    #[tokio::test]
    async fn serialized_stop_and_cancel_only_take_session_once() {
        use std::sync::Arc;

        let state = Arc::new(make_state_with_session().await);
        let cancel_state = state.clone();
        let cancel_task = tokio::spawn(async move {
            let _guard = cancel_state.session_action.lock().await;
            cancel_session_inner(&cancel_state).await.cancelled
        });
        let stop_state = state.clone();
        let stop_task = tokio::spawn(async move {
            let _guard = stop_state.session_action.lock().await;
            if matches!(
                *stop_state.recording_state.lock().await,
                RecordingState::Recording
            ) {
                let _ = stop_session_inner(&stop_state, &NoOpInjector, None).await;
                true
            } else {
                false
            }
        });

        let (cancelled, stopped) = tokio::join!(cancel_task, stop_task);
        assert_ne!(cancelled.unwrap(), stopped.unwrap());
        assert!(state.session.lock().await.is_none());
        assert!(matches!(
            *state.recording_state.lock().await,
            RecordingState::Idle
        ));
    }
}
