use tauri::{Emitter, State};

use crate::{
    audio,
    session_service::{self, ClipboardInjector},
    settings::{self, AppSettings},
    state::{AppState, Mode, RecordingState},
    tray,
};

/// Rust → 全ウィンドウへ配信するセッション状態イベント。
#[derive(Clone, serde::Serialize)]
struct SessionUiEvent {
    state: RecordingState,
    mode: Mode,
    final_text: Option<String>,
    error: Option<String>,
}

#[tauri::command]
pub async fn get_mode(state: State<'_, AppState>) -> Result<Mode, String> {
    Ok(state.mode.lock().await.clone())
}

#[tauri::command]
pub async fn set_mode(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mode: Mode,
) -> Result<(), String> {
    *state.mode.lock().await = mode.clone();
    // モードをストアに永続化
    let mut s = state.settings.lock().await.clone();
    s.mode = mode.clone();
    settings::save(&app, &s).map_err(|e| e.to_string())?;
    *state.settings.lock().await = s;
    // トレイのモード表示を更新
    let mode_str = if matches!(mode, Mode::Polish) { "Polish" } else { "Raw" };
    let recording_state = state.recording_state.lock().await.clone();
    let status_str = match recording_state {
        RecordingState::Recording => "録音中 ●",
        RecordingState::Processing => "処理中",
        RecordingState::Idle => "待機中",
    };
    tray::update_status(&app, status_str, mode_str);
    Ok(())
}

#[tauri::command]
pub async fn get_recording_state(state: State<'_, AppState>) -> Result<RecordingState, String> {
    Ok(state.recording_state.lock().await.clone())
}

/// F4 押下: WASAPI キャプチャを開始する。
#[tauri::command]
pub async fn start_recording_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (level_tx, mut level_rx) = tokio::sync::mpsc::unbounded_channel::<f32>();
    session_service::start_session_inner(&state, Some(level_tx)).await?;

    let mode = state.mode.lock().await.clone();
    let mode_str = if matches!(mode, Mode::Polish) { "Polish" } else { "Raw" };
    tray::update_status(&app, "録音中 ●", mode_str);
    let _ = app.emit("session://state-changed", SessionUiEvent {
        state: RecordingState::Recording,
        mode,
        final_text: None,
        error: None,
    });

    // 音量レベルを FloatingBar に転送するタスク（WasapiInput が drop されると自動終了）
    let app_level = app.clone();
    tokio::spawn(async move {
        while let Some(level) = level_rx.recv().await {
            let _ = app_level.emit("audio://level", level);
        }
    });

    Ok(())
}

/// F4 離す: 録音停止 → ASR → テキスト注入。
#[tauri::command]
pub async fn stop_recording_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Processing 状態を全ウィンドウに通知
    {
        let mode = state.mode.lock().await.clone();
        let _ = app.emit("session://state-changed", SessionUiEvent {
            state: RecordingState::Processing,
            mode,
            final_text: None,
            error: None,
        });
    }

    let result = session_service::stop_session_inner(&state, &ClipboardInjector).await;

    let mode = state.mode.lock().await.clone();
    let mode_str = if matches!(mode, Mode::Polish) { "Polish" } else { "Raw" };
    let status_str = if result.is_ok() { "待機中" } else { "エラー ✕" };
    tray::update_status(&app, status_str, mode_str);
    let _ = app.emit("session://state-changed", SessionUiEvent {
        state: RecordingState::Idle,
        mode,
        final_text: result.as_ref().ok().filter(|s| !s.is_empty()).cloned(),
        error: result.as_ref().err().cloned(),
    });

    result
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
pub async fn list_audio_devices() -> Result<Vec<audio::AudioDeviceInfo>, String> {
    #[cfg(target_os = "windows")]
    return audio::wasapi::list_capture_devices().map_err(|e| e.to_string());
    #[cfg(not(target_os = "windows"))]
    Ok(vec![])
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
