use tauri::State;

use crate::{
    audio,
    session_service::{self, ClipboardInjector},
    settings::{self, AppSettings},
    state::{AppState, Mode, RecordingState},
    tray,
};

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
    s.mode = mode;
    settings::save(&app, &s).map_err(|e| e.to_string())?;
    *state.settings.lock().await = s;
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
    session_service::start_session_inner(&state).await?;

    let mode = state.mode.lock().await.clone();
    let mode_str = if matches!(mode, Mode::Polish) { "Polish" } else { "Raw" };
    tray::update_status(&app, "録音中 ●", mode_str);
    Ok(())
}

/// F4 離す: 録音停止 → ASR → テキスト注入。
#[tauri::command]
pub async fn stop_recording_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let result = session_service::stop_session_inner(&state, &ClipboardInjector).await;

    let mode = state.mode.lock().await.clone();
    let mode_str = if matches!(mode, Mode::Polish) { "Polish" } else { "Raw" };
    let status_str = if result.is_ok() { "待機中" } else { "エラー ✕" };
    tray::update_status(&app, status_str, mode_str);

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
