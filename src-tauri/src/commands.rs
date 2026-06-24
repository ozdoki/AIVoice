use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tauri::{Emitter, State};

use crate::{
    audio,
    context::{self, FocusedAppContext},
    hotkey::{self, HotkeySet},
    local_data::{self, HistoryEntry, SessionMetrics, UsageDaySummary, MAX_DICTIONARY_WORDS},
    session_service::{self, ClipboardInjector},
    settings::{self, AppSettings},
    state::{AppState, Mode, RecordingState, RecordingTrigger},
    tray,
};

#[derive(Clone, serde::Serialize)]
struct SessionUiEvent {
    state: RecordingState,
    mode: Mode,
    final_text: Option<String>,
    history_id: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelItem>,
}

#[derive(Deserialize)]
struct ModelItem {
    id: String,
}

const API_KEY_IMPORT_MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, PartialEq)]
enum ShortcutAction {
    Start(RecordingTrigger),
    Stop,
    Ignore,
}

fn push_to_talk_down_action(recording_state: &RecordingState) -> ShortcutAction {
    match recording_state {
        RecordingState::Idle => ShortcutAction::Start(RecordingTrigger::PushToTalk),
        RecordingState::Recording => ShortcutAction::Stop,
        RecordingState::Processing => ShortcutAction::Ignore,
    }
}

fn push_to_talk_up_action(
    recording_state: &RecordingState,
    trigger: Option<&RecordingTrigger>,
) -> ShortcutAction {
    if matches!(recording_state, RecordingState::Recording)
        && matches!(trigger, Some(RecordingTrigger::PushToTalk))
    {
        ShortcutAction::Stop
    } else {
        ShortcutAction::Ignore
    }
}

fn hands_free_action(recording_state: &RecordingState) -> ShortcutAction {
    match recording_state {
        RecordingState::Idle => ShortcutAction::Start(RecordingTrigger::HandsFree),
        RecordingState::Recording => ShortcutAction::Stop,
        RecordingState::Processing => ShortcutAction::Ignore,
    }
}

fn parse_model_ids(body: &str) -> Result<Vec<String>, String> {
    let response: ModelsResponse = serde_json::from_str(body)
        .map_err(|error| format!("モデル一覧のJSONが不正です: {error}"))?;
    let ids: BTreeSet<_> = response
        .data
        .into_iter()
        .map(|model| model.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    if ids.is_empty() {
        return Err("利用可能なモデルが見つかりませんでした。".to_string());
    }
    Ok(ids.into_iter().collect())
}

fn hotkey_set(settings: &AppSettings) -> HotkeySet {
    HotkeySet::new(
        settings.push_to_talk_hotkey.clone(),
        settings.hands_free_hotkey.clone(),
        settings.toggle_mode_hotkey.clone(),
    )
}

fn settings_for_ui(settings: &AppSettings) -> Result<serde_json::Value, String> {
    let has_api_key = !settings.api_key.is_empty();
    let mut value = serde_json::to_value(settings).map_err(|error| error.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "設定を画面用に変換できませんでした。".to_string())?;
    object.insert(
        "api_key".to_string(),
        serde_json::Value::String(String::new()),
    );
    object.insert(
        "has_api_key".to_string(),
        serde_json::Value::Bool(has_api_key),
    );
    Ok(value)
}

fn select_api_key(stored_api_key: &str, override_api_key: Option<String>) -> String {
    override_api_key
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| stored_api_key.to_string())
}

fn canonical_workspace_roots() -> Vec<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let mut roots = vec![cwd.clone()];
    if cwd
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("src-tauri"))
    {
        if let Some(parent) = cwd.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    roots
}

fn is_under_workspace(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

fn resolve_api_key_import_path(path: &str) -> Result<PathBuf, String> {
    let path = path.trim().trim_matches('"');
    if path.is_empty() {
        return Err("Import file path is empty.".to_string());
    }

    let roots = canonical_workspace_roots();
    let raw = PathBuf::from(path);
    let candidates: Vec<PathBuf> = if raw.is_absolute() {
        vec![raw]
    } else {
        roots.iter().map(|root| root.join(&raw)).collect()
    };

    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("Import file path could not be resolved: {error}"))?;
        if !is_under_workspace(&canonical, &roots) {
            return Err("Import file must be inside this workspace.".to_string());
        }
        return Ok(canonical);
    }

    Err("Import file was not found inside this workspace.".to_string())
}

fn parse_env_value(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].trim().to_string();
        }
    }
    value.to_string()
}

fn extract_openai_api_key_env(contents: &str) -> Result<String, String> {
    for line in contents.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != "OPENAI_API_KEY" {
            continue;
        }
        let api_key = parse_env_value(value);
        if api_key.is_empty() {
            return Err("OPENAI_API_KEY is empty.".to_string());
        }
        return Ok(api_key);
    }

    Err("OPENAI_API_KEY was not found in the import file.".to_string())
}

fn read_api_key_from_env_file(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Import file metadata could not be read: {error}"))?;
    if !metadata.is_file() {
        return Err("Import path must point to a file.".to_string());
    }
    if metadata.len() > API_KEY_IMPORT_MAX_BYTES {
        return Err("Import file is too large.".to_string());
    }
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Import file could not be read as UTF-8: {error}"))?;
    extract_openai_api_key_env(&contents)
}

fn cleanup_api_key_import_file(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|error| {
        format!("Imported API key was saved, but the temporary import file could not be deleted: {error}")
    })
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
    let previous = state.settings.lock().await.clone();
    let mut next = previous.clone();
    next.mode = mode.clone();
    settings::save(&app, &next).map_err(|error| error.to_string())?;
    *state.settings.lock().await = next;
    *state.mode.lock().await = mode.clone();
    let current_settings = state.settings.lock().await.clone();
    if let Ok(payload) = settings_for_ui(&current_settings) {
        let _ = app.emit("settings://changed", payload);
    }

    let mode_label = if matches!(mode, Mode::Polish) {
        "Polish"
    } else {
        "Raw"
    };
    let status_label = match *state.recording_state.lock().await {
        RecordingState::Recording => "録音中",
        RecordingState::Processing => "処理中",
        RecordingState::Idle => "待機中",
    };
    tray::update_status(&app, status_label, mode_label);
    Ok(())
}

#[tauri::command]
pub async fn get_recording_state(state: State<'_, AppState>) -> Result<RecordingState, String> {
    Ok(state.recording_state.lock().await.clone())
}

async fn start_recording_locked(
    app: &tauri::AppHandle,
    state: &AppState,
    trigger: RecordingTrigger,
) -> Result<(), String> {
    if !matches!(*state.recording_state.lock().await, RecordingState::Idle) {
        return Ok(());
    }

    let (level_tx, mut level_rx) = tokio::sync::mpsc::unbounded_channel::<f32>();
    session_service::start_session_inner(state, Some(level_tx)).await?;
    *state.recording_trigger.lock().await = Some(trigger);

    let mode = state.mode.lock().await.clone();
    let mode_label = if matches!(mode, Mode::Polish) {
        "Polish"
    } else {
        "Raw"
    };
    tray::update_status(app, "録音中", mode_label);
    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Recording,
            mode,
            final_text: None,
            history_id: None,
            error: None,
        },
    );

    let app_level = app.clone();
    tokio::spawn(async move {
        while let Some(level) = level_rx.recv().await {
            let _ = app_level.emit("audio://level", level);
        }
    });
    Ok(())
}

async fn stop_recording_locked(app: &tauri::AppHandle, state: &AppState) -> Result<String, String> {
    if !matches!(
        *state.recording_state.lock().await,
        RecordingState::Recording
    ) {
        return Ok(String::new());
    }

    let mode = state.mode.lock().await.clone();
    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Processing,
            mode: mode.clone(),
            final_text: None,
            history_id: None,
            error: None,
        },
    );

    let result = session_service::stop_session_inner(state, &ClipboardInjector).await;
    *state.recording_trigger.lock().await = None;

    let mode_label = if matches!(mode, Mode::Polish) {
        "Polish"
    } else {
        "Raw"
    };
    tray::update_status(
        app,
        if result.is_ok() {
            "待機中"
        } else {
            "エラー"
        },
        mode_label,
    );
    let history_id = match &result {
        Ok(outcome) if !outcome.final_text.is_empty() => {
            let entry = local_data::make_history_entry(
                outcome.raw_text.clone(),
                outcome.final_text.clone(),
                outcome.mode.clone(),
                outcome.duration_ms,
                None,
            );
            let saved_id = local_data::append_history(app, entry)
                .map(|entry| entry.id)
                .map_err(|error| tracing::warn!("failed to save history: {error}"))
                .ok();
            let settings = state.settings.lock().await.clone();
            if let Err(error) = local_data::record_usage(
                app,
                SessionMetrics {
                    mode: outcome.mode.clone(),
                    raw_text: outcome.raw_text.clone(),
                    final_text: outcome.final_text.clone(),
                    duration_ms: outcome.duration_ms,
                    api_model: settings.api_model,
                    polish_model: settings.polish_model,
                },
            ) {
                tracing::warn!("failed to record usage: {error}");
            }
            saved_id
        }
        Err(error) => {
            let entry = local_data::make_history_entry(
                String::new(),
                String::new(),
                mode.clone(),
                0,
                Some(error.clone()),
            );
            local_data::append_history(app, entry)
                .map(|entry| entry.id)
                .map_err(|save_error| tracing::warn!("failed to save error history: {save_error}"))
                .ok()
        }
        _ => None,
    };

    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Idle,
            mode,
            final_text: result
                .as_ref()
                .ok()
                .map(|outcome| outcome.final_text.clone())
                .filter(|text| !text.is_empty()),
            history_id,
            error: result.as_ref().err().cloned(),
        },
    );
    result.map(|outcome| outcome.final_text)
}

#[tauri::command]
pub async fn start_recording_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.session_action.lock().await;
    start_recording_locked(&app, &state, RecordingTrigger::Manual).await
}

#[tauri::command]
pub async fn stop_recording_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _guard = state.session_action.lock().await;
    stop_recording_locked(&app, &state).await
}

#[tauri::command]
pub async fn push_to_talk_down(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.session_action.lock().await;
    let recording_state = state.recording_state.lock().await.clone();
    match push_to_talk_down_action(&recording_state) {
        ShortcutAction::Start(trigger) => start_recording_locked(&app, &state, trigger).await,
        ShortcutAction::Stop => {
            stop_recording_locked(&app, &state).await?;
            Ok(())
        }
        ShortcutAction::Ignore => Ok(()),
    }
}

#[tauri::command]
pub async fn push_to_talk_up(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.session_action.lock().await;
    let recording_state = state.recording_state.lock().await.clone();
    let trigger = state.recording_trigger.lock().await.clone();
    if matches!(
        push_to_talk_up_action(&recording_state, trigger.as_ref()),
        ShortcutAction::Stop
    ) {
        stop_recording_locked(&app, &state).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_hands_free_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.session_action.lock().await;
    let recording_state = state.recording_state.lock().await.clone();
    match hands_free_action(&recording_state) {
        ShortcutAction::Start(trigger) => start_recording_locked(&app, &state, trigger).await,
        ShortcutAction::Stop => {
            stop_recording_locked(&app, &state).await?;
            Ok(())
        }
        ShortcutAction::Ignore => Ok(()),
    }
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().await.clone();
    settings_for_ui(&settings)
}

async fn list_models_with_key(base_url: String, api_key: String) -> Result<Vec<String>, String> {
    let url = format!("{}/models", base_url.trim().trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut request = client.get(url);
    if !api_key.trim().is_empty() {
        request = request.bearer_auth(api_key.trim());
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("モデル一覧を取得できませんでした: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("モデル一覧の応答を読み取れませんでした: {error}"))?;
    if !status.is_success() {
        return Err(format!("モデル一覧の取得に失敗しました (HTTP {status})"));
    }
    parse_model_ids(&body)
}

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    let stored_api_key = state.settings.lock().await.api_key.clone();
    let effective_api_key = select_api_key(&stored_api_key, Some(api_key));
    list_models_with_key(base_url, effective_api_key).await
}

#[tauri::command]
pub async fn test_api_connection(
    state: State<'_, AppState>,
    base_url: String,
    api_key_override: Option<String>,
) -> Result<usize, String> {
    let stored_api_key = state.settings.lock().await.api_key.clone();
    let effective_api_key = select_api_key(&stored_api_key, api_key_override);
    let models = list_models_with_key(base_url, effective_api_key).await?;
    Ok(models.len())
}

#[tauri::command]
pub async fn list_audio_devices() -> Result<Vec<audio::AudioDeviceInfo>, String> {
    #[cfg(target_os = "windows")]
    return audio::wasapi::list_capture_devices().map_err(|error| error.to_string());
    #[cfg(not(target_os = "windows"))]
    Ok(vec![])
}

#[tauri::command]
pub async fn copy_text(text: String) -> Result<(), String> {
    if text.is_empty() {
        return Err("コピーするテキストがありません。".to_string());
    }
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("クリップボードを開けません: {error}"))?;
    clipboard
        .set_text(text)
        .map_err(|error| format!("クリップボードへコピーできません: {error}"))
}

#[tauri::command]
pub async fn inject_text(text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("注入するテキストがありません。".to_string());
    }
    crate::inject::inject_text(&text).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_history(app: tauri::AppHandle) -> Result<Vec<HistoryEntry>, String> {
    local_data::load_history(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_history_item(
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<HistoryEntry>, String> {
    local_data::delete_history_item(&app, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn clear_history(app: tauri::AppHandle) -> Result<(), String> {
    local_data::clear_history(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_dictionary(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    local_data::load_dictionary(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn add_dictionary_word(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    word: String,
) -> Result<Vec<String>, String> {
    let words = local_data::add_dictionary_word(&app, &word).map_err(|error| error.to_string())?;
    *state.dictionary_words.lock().await = words.clone();
    Ok(words)
}

#[tauri::command]
pub async fn remove_dictionary_word(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    word: String,
) -> Result<Vec<String>, String> {
    let words =
        local_data::remove_dictionary_word(&app, &word).map_err(|error| error.to_string())?;
    *state.dictionary_words.lock().await = words.clone();
    Ok(words)
}

#[tauri::command]
pub async fn get_usage_summary(app: tauri::AppHandle) -> Result<Vec<UsageDaySummary>, String> {
    local_data::load_usage(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_focused_app_context() -> Result<Option<FocusedAppContext>, String> {
    Ok(context::focused_app_context())
}

#[tauri::command]
pub async fn dictionary_limit() -> Result<usize, String> {
    Ok(MAX_DICTIONARY_WORDS)
}

#[tauri::command]
pub async fn save_api_key(
    state: State<'_, AppState>,
    api_key: String,
) -> Result<serde_json::Value, String> {
    let api_key = api_key.trim().to_string();
    settings::store_api_key(&api_key).map_err(|error| error.to_string())?;
    let mut current = state.settings.lock().await;
    current.api_key = api_key;
    settings_for_ui(&current)
}

#[tauri::command]
pub async fn delete_api_key(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    settings::delete_api_key().map_err(|error| error.to_string())?;
    let mut current = state.settings.lock().await;
    current.api_key.clear();
    settings_for_ui(&current)
}

#[tauri::command]
pub async fn import_api_key_from_env_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<serde_json::Value, String> {
    let resolved_path = resolve_api_key_import_path(&path)?;
    let api_key = read_api_key_from_env_file(&resolved_path)?;
    let previous_api_key = state.settings.lock().await.api_key.clone();

    settings::store_api_key(&api_key).map_err(|error| error.to_string())?;

    if let Err(cleanup_error) = cleanup_api_key_import_file(&resolved_path) {
        let restore_result = if previous_api_key.is_empty() {
            settings::delete_api_key()
        } else {
            settings::store_api_key(&previous_api_key)
        };
        return Err(match restore_result {
            Ok(()) => cleanup_error,
            Err(restore_error) => format!(
                "{cleanup_error}. Previous Credential Manager value could not be restored: {restore_error}"
            ),
        });
    }

    let mut current = state.settings.lock().await;
    current.api_key = api_key;
    settings_for_ui(&current)
}

#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mut new_settings: AppSettings,
) -> Result<(), String> {
    let previous = state.settings.lock().await.clone();
    if new_settings.api_key.is_empty() && !previous.api_key.is_empty() {
        new_settings.api_key = previous.api_key.clone();
    }
    let normalized = hotkey::reconfigure_hotkeys(hotkey_set(&new_settings))?;
    new_settings.push_to_talk_hotkey = normalized.push_to_talk;
    new_settings.hands_free_hotkey = normalized.hands_free;
    new_settings.toggle_mode_hotkey = normalized.toggle_mode;
    if previous.launch_at_login != new_settings.launch_at_login {
        if let Err(error) = crate::startup::set_launch_at_login(new_settings.launch_at_login) {
            let _ = hotkey::reconfigure_hotkeys(hotkey_set(&previous));
            return Err(format!("ログイン時起動の設定を変更できませんでした: {error}"));
        }
    }

    if let Err(error) = settings::save(&app, &new_settings) {
        let restore_error = hotkey::reconfigure_hotkeys(hotkey_set(&previous)).err();
        let _ = settings::save(&app, &previous);
        return Err(match restore_error {
            Some(restore_error) => format!(
                "設定を保存できませんでした: {error}。以前のショートカットの復元にも失敗しました: {restore_error}"
            ),
            None => format!("設定を保存できませんでした: {error}"),
        });
    }

    *state.mode.lock().await = new_settings.mode.clone();
    *state.settings.lock().await = new_settings;
    let current = state.settings.lock().await.clone();
    if let Ok(payload) = settings_for_ui(&current) {
        let _ = app.emit("settings://changed", payload);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use super::{
        cleanup_api_key_import_file, extract_openai_api_key_env, hands_free_action,
        list_models_with_key, parse_model_ids, push_to_talk_down_action, push_to_talk_up_action,
        read_api_key_from_env_file, select_api_key, settings_for_ui, ShortcutAction,
    };
    use crate::{
        settings::{AppSettings, HotkeyBinding},
        state::{Mode, RecordingState, RecordingTrigger},
    };

    fn serve_once(status: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            stream.write_all(response.as_bytes()).unwrap();
        });
        format!("http://{address}")
    }

    #[test]
    fn model_ids_are_trimmed_deduplicated_and_sorted() {
        let result =
            parse_model_ids(r#"{"data":[{"id":"z-model"},{"id":" a-model "},{"id":"z-model"}]}"#)
                .unwrap();
        assert_eq!(result, vec!["a-model", "z-model"]);
    }

    #[test]
    fn empty_model_list_is_an_error() {
        assert!(parse_model_ids(r#"{"data":[]}"#).is_err());
        assert!(parse_model_ids(r#"{"data":[{"id":" "} ]}"#).is_err());
    }

    #[test]
    fn invalid_model_response_is_an_error() {
        assert!(parse_model_ids("not-json").is_err());
        assert!(parse_model_ids(r#"{"models":[]}"#).is_err());
    }

    #[test]
    fn settings_for_ui_hides_api_key_and_reports_presence() {
        let settings = AppSettings {
            api_key: "sk-test-secret".to_string(),
            push_to_talk_hotkey: HotkeyBinding::push_to_talk_default(),
            hands_free_hotkey: HotkeyBinding::hands_free_default(),
            toggle_mode_hotkey: HotkeyBinding::toggle_mode_default(),
            mode: Mode::Raw,
            ..Default::default()
        };
        let payload = settings_for_ui(&settings).unwrap();
        assert_eq!(payload.get("api_key").unwrap(), "");
        assert_eq!(payload.get("has_api_key").unwrap(), true);
        assert!(!payload.to_string().contains("sk-test-secret"));
    }

    #[test]
    fn api_key_override_takes_precedence_over_stored_key() {
        assert_eq!(
            select_api_key("stored-key", Some(" override-key ".to_string())),
            "override-key"
        );
        assert_eq!(
            select_api_key("stored-key", Some(" ".to_string())),
            "stored-key"
        );
        assert_eq!(select_api_key("stored-key", None), "stored-key");
    }

    #[test]
    fn openai_api_key_env_is_extracted_without_exposing_other_values() {
        let contents = r#"
# ignored
OTHER_SECRET=do-not-read
export OPENAI_API_KEY = "sk-test-import"
"#;
        assert_eq!(
            extract_openai_api_key_env(contents).unwrap(),
            "sk-test-import"
        );
        assert!(extract_openai_api_key_env("OPENAI_API_KEY=").is_err());
        assert!(extract_openai_api_key_env("OTHER_SECRET=value").is_err());
    }

    #[test]
    fn api_key_import_file_is_read_and_removed() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("aivoice-import-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("openai-key.env");
        std::fs::write(&path, "OPENAI_API_KEY=sk-test-import\n").unwrap();

        assert_eq!(read_api_key_from_env_file(&path).unwrap(), "sk-test-import");
        cleanup_api_key_import_file(&path).unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn list_models_reads_openai_compatible_response() {
        let base_url = serve_once(
            "200 OK",
            r#"{"data":[{"id":"model-b"},{"id":"model-a"},{"id":"model-b"}]}"#,
        );
        let models = list_models_with_key(base_url, "test-key".to_string())
            .await
            .unwrap();
        assert_eq!(models, vec!["model-a", "model-b"]);
    }

    #[tokio::test]
    async fn list_models_reports_http_and_json_errors() {
        let unauthorized = serve_once("401 Unauthorized", r#"{"error":"unauthorized"}"#);
        assert!(list_models_with_key(unauthorized, String::new())
            .await
            .unwrap_err()
            .contains("401"));

        let invalid = serve_once("200 OK", "not-json");
        assert!(list_models_with_key(invalid, String::new()).await.is_err());
    }

    #[tokio::test]
    async fn list_models_reports_connection_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        assert!(
            list_models_with_key(format!("http://{address}"), String::new())
                .await
                .is_err()
        );
    }

    #[test]
    fn shortcut_actions_cover_start_stop_and_release_deduplication() {
        assert_eq!(
            push_to_talk_down_action(&RecordingState::Idle),
            ShortcutAction::Start(RecordingTrigger::PushToTalk)
        );
        assert_eq!(
            hands_free_action(&RecordingState::Idle),
            ShortcutAction::Start(RecordingTrigger::HandsFree)
        );
        assert_eq!(
            hands_free_action(&RecordingState::Recording),
            ShortcutAction::Stop
        );
        assert_eq!(
            push_to_talk_down_action(&RecordingState::Recording),
            ShortcutAction::Stop
        );
        assert_eq!(
            push_to_talk_up_action(
                &RecordingState::Recording,
                Some(&RecordingTrigger::PushToTalk)
            ),
            ShortcutAction::Stop
        );
        assert_eq!(
            push_to_talk_up_action(&RecordingState::Idle, None),
            ShortcutAction::Ignore
        );
        assert_eq!(
            push_to_talk_up_action(
                &RecordingState::Recording,
                Some(&RecordingTrigger::HandsFree)
            ),
            ShortcutAction::Ignore
        );
    }
}
