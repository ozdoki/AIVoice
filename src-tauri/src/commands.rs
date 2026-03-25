use tokio::sync::watch;
use tauri::State;

use crate::{
    inject,
    mode,
    settings::{self, AppSettings},
    speech::{
        mock::MockSpeechProvider,
        openai_compatible::OpenAiCompatibleProvider,
        SpeechProvider,
    },
    state::{AppState, Mode, RecordingState, SessionController},
};

#[tauri::command]
pub async fn get_mode(state: State<'_, AppState>) -> Result<Mode, String> {
    Ok(state.mode.lock().await.clone())
}

#[tauri::command]
pub async fn set_mode(state: State<'_, AppState>, mode: Mode) -> Result<(), String> {
    *state.mode.lock().await = mode;
    Ok(())
}

#[tauri::command]
pub async fn get_recording_state(state: State<'_, AppState>) -> Result<RecordingState, String> {
    Ok(state.recording_state.lock().await.clone())
}

/// F4 押下: WASAPI キャプチャを開始する。
/// 既にセッションが存在する場合は無視する。
#[tauri::command]
pub async fn start_recording_session(state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state.session.lock().await;
    if session.is_some() {
        return Ok(());
    }

    let (stop_tx, stop_rx) = watch::channel(false);
    let input = state.audio_input.clone();
    let capture_task =
        tokio::task::spawn_blocking(move || input.capture_blocking(stop_rx));

    *session = Some(SessionController { stop_tx, capture_task });
    *state.recording_state.lock().await = RecordingState::Recording;
    Ok(())
}

/// F4 離す: 録音停止 → ASR → テキスト注入。
/// セッションがなければ空文字を返す。
#[tauri::command]
pub async fn stop_recording_session(state: State<'_, AppState>) -> Result<String, String> {
    let controller = state.session.lock().await.take();
    let Some(controller) = controller else {
        return Ok(String::new());
    };

    *state.recording_state.lock().await = RecordingState::Processing;
    let _ = controller.stop_tx.send(true);

    let audio = controller
        .capture_task
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let current_settings = state.settings.lock().await.clone();
    let raw_text: String = if current_settings.api_key.is_empty() {
        MockSpeechProvider::default()
            .transcribe(&audio)
            .await
            .map_err(|e| e.to_string())?
    } else {
        OpenAiCompatibleProvider {
            base_url: current_settings.api_base_url,
            api_key: current_settings.api_key,
            model: current_settings.api_model,
        }
        .transcribe(&audio)
        .await
        .map_err(|e| e.to_string())?
    };

    let current_mode = state.mode.lock().await.clone();
    let current_settings_for_mode = state.settings.lock().await.clone();
    let final_text = mode::route(&current_mode, &current_settings_for_mode, &raw_text).await;

    inject::inject_text_after_f4(&final_text).map_err(|e| e.to_string())?;

    *state.recording_state.lock().await = RecordingState::Idle;
    Ok(final_text)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    new_settings: AppSettings,
) -> Result<(), String> {
    settings::save(&app, &new_settings).map_err(|e| e.to_string())?;
    *state.settings.lock().await = new_settings;
    Ok(())
}
