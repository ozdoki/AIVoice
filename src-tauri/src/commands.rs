use tauri::State;

use crate::{
    inject::inject_text,
    mode,
    settings::{self, AppSettings},
    speech::mock::MockSpeechProvider,
    speech::openai_compatible::OpenAiCompatibleProvider,
    speech::SpeechProvider,
    state::{AppState, Mode, RecordingState},
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

/// API Key が設定されていれば OpenAI 互換 ASR を使い、未設定なら Mock を使う。
/// 音声データはまだ WASAPI 未実装のため空スライスを渡す。
#[tauri::command]
pub async fn start_mock_session(state: State<'_, AppState>) -> Result<String, String> {
    {
        *state.recording_state.lock().await = RecordingState::Processing;
    }

    let current_settings = state.settings.lock().await.clone();

    let raw_text: String = if current_settings.api_key.is_empty() {
        MockSpeechProvider::default()
            .transcribe(&[])
            .await
            .map_err(|e| e.to_string())?
    } else {
        OpenAiCompatibleProvider {
            base_url: current_settings.api_base_url.clone(),
            api_key: current_settings.api_key.clone(),
            model: current_settings.api_model.clone(),
        }
        .transcribe(&[])
        .await
        .map_err(|e| e.to_string())?
    };

    let current_mode = state.mode.lock().await.clone();
    let final_text = mode::route(&current_mode, &raw_text);

    inject_text(&final_text).map_err(|e| e.to_string())?;

    {
        *state.recording_state.lock().await = RecordingState::Idle;
    }

    Ok(final_text)
}

#[tauri::command]
pub async fn stop_session(state: State<'_, AppState>) -> Result<(), String> {
    *state.recording_state.lock().await = RecordingState::Idle;
    Ok(())
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
