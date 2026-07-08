use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tauri::{Emitter, State};

use crate::{
    audio,
    context::{self, FocusedAppContext, FocusedWindowTarget},
    hotkey::{self, HotkeySet},
    local_data::{
        self, DictionarySuggestion, HistoryEntry, SessionMetrics, SnippetEntry, UsageDaySummary,
        MAX_DICTIONARY_WORDS, MAX_SNIPPETS,
    },
    mode,
    recovery::{self, RecoverySessionSummary},
    session_service,
    settings::{self, AppSettings},
    speech::{
        openai_compatible::OpenAiCompatibleProvider,
        realtime::{
            supports_realtime_model, transcribe_realtime, RealtimeStatus,
            REALTIME_TRANSCRIPTION_MODEL,
        },
        SpeechProvider,
    },
    state::{AppState, Mode, RecordingState, RecordingTrigger},
    tray,
};

#[derive(Clone, serde::Serialize)]
struct SessionUiEvent {
    state: RecordingState,
    mode: Mode,
    polish_preset: Option<String>,
    phase: String,
    raw_text: Option<String>,
    final_text: Option<String>,
    history_id: Option<String>,
    error: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct PartialTextEvent {
    text: String,
}

#[derive(Clone, serde::Serialize)]
struct LiveTranscriptStatusEvent {
    state: String,
    detail: Option<String>,
}

struct TargetWindowInjector {
    target: Option<FocusedWindowTarget>,
}

impl session_service::TextInjector for TargetWindowInjector {
    fn inject(&self, text: &str) -> anyhow::Result<()> {
        if let Some(target) = &self.target {
            crate::inject::inject_text_to_window(text, target)
        } else {
            crate::inject::inject_text(text)
        }
    }
}

struct PreviewInjector;

impl session_service::TextInjector for PreviewInjector {
    fn inject(&self, _text: &str) -> anyhow::Result<()> {
        Ok(())
    }
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

fn trigger_label(trigger: &RecordingTrigger) -> String {
    match trigger {
        RecordingTrigger::PushToTalk => "push_to_talk",
        RecordingTrigger::HandsFree => "hands_free",
        RecordingTrigger::Manual => "manual",
    }
    .to_string()
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
        settings.hands_free_raw_hotkey.clone(),
        settings.hands_free_polish_hotkey.clone(),
    )
}

fn should_start_realtime_asr(settings: &AppSettings, session_mode: &Mode) -> bool {
    !settings.api_key.trim().is_empty()
        && (settings.show_live_transcript_in_floating_bar || !matches!(session_mode, Mode::Raw))
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
    mode_override: Option<Mode>,
) -> Result<(), String> {
    if !matches!(*state.recording_state.lock().await, RecordingState::Idle) {
        return Ok(());
    }

    let focused_target = context::current_external_focused_window();
    if let Some(target) = &focused_target {
        *state.last_target_window.lock().await = Some(target.clone());
    }

    let (level_tx, mut level_rx) = tokio::sync::mpsc::unbounded_channel::<f32>();
    let settings = state.settings.lock().await.clone();
    let session_mode = match mode_override {
        Some(mode) => mode,
        None => state.mode.lock().await.clone(),
    };
    let focused_context_for_preset = focused_target.as_ref().map(|target| FocusedAppContext {
        process_name: target.process_name.clone(),
        window_title: target.window_title.clone(),
    });
    let session_polish_preset = if matches!(session_mode, Mode::Polish) {
        context::suggested_polish_preset(
            focused_context_for_preset.as_ref(),
            &settings.polish_preset,
        )
    } else {
        settings.polish_preset.clone()
    };
    let (partial_tx, mut partial_rx) = if settings.show_live_transcript_in_floating_bar {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let (status_tx, mut status_rx) = if settings.show_live_transcript_in_floating_bar {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<RealtimeStatus>();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let should_start_realtime = should_start_realtime_asr(&settings, &session_mode);
    let realtime_model = if !should_start_realtime {
        None
    } else if supports_realtime_model(&settings.api_model) {
        Some(settings.api_model.clone())
    } else if settings.show_live_transcript_in_floating_bar {
        Some(REALTIME_TRANSCRIPTION_MODEL.to_string())
    } else {
        None
    };
    let (chunk_tx, realtime_task) = if let Some(realtime_model) = realtime_model {
        tracing::info!(
            configured_model = %settings.api_model,
            realtime_model = %realtime_model,
            live_transcript = settings.show_live_transcript_in_floating_bar,
            mode = ?session_mode,
            "starting recording with realtime ASR"
        );
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(transcribe_realtime(
            settings.api_base_url.clone(),
            settings.api_key.clone(),
            realtime_model,
            chunk_rx,
            partial_tx,
            status_tx,
        ));
        (Some(chunk_tx), Some(task))
    } else {
        tracing::info!(
            model = %settings.api_model,
            has_api_key = !settings.api_key.trim().is_empty(),
            live_transcript = settings.show_live_transcript_in_floating_bar,
            mode = ?session_mode,
            "starting recording without realtime ASR"
        );
        if settings.show_live_transcript_in_floating_bar {
            let detail = if settings.api_key.trim().is_empty() {
                "APIキーが未設定のため、録音中の文字表示を開始できません。".to_string()
            } else {
                format!(
                    "ASR Model {} は録音中の文字表示に未対応です。",
                    settings.api_model
                )
            };
            if let Some(tx) = &status_tx {
                let _ = tx.send(RealtimeStatus {
                    state: "error".to_string(),
                    detail: Some(detail),
                });
            }
        }
        (None, None)
    };
    let recovery_session =
        recovery::create_session(app, session_mode.clone(), trigger_label(&trigger))
            .map_err(|error| error.to_string())?;
    if let Err(error) = session_service::start_session_inner(
        state,
        Some(level_tx),
        chunk_tx,
        realtime_task,
        Some(recovery_session.id.clone()),
        Some(recovery_session.audio_path.clone()),
        session_mode.clone(),
        session_polish_preset.clone(),
    )
    .await
    {
        let _ = recovery::delete_session(app, &recovery_session.id);
        return Err(error);
    }
    *state.recording_trigger.lock().await = Some(trigger);

    let mode_label = if matches!(session_mode, Mode::Polish) {
        "Polish"
    } else {
        "Raw"
    };
    tray::update_status(app, "録音中", mode_label);
    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Recording,
            mode: session_mode,
            polish_preset: Some(session_polish_preset),
            phase: "recording".to_string(),
            raw_text: None,
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
    if let Some(mut rx) = partial_rx.take() {
        let app_partial = app.clone();
        let _ = app_partial.emit(
            "session://partial-text",
            PartialTextEvent {
                text: String::new(),
            },
        );
        tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                let _ = app_partial.emit("session://partial-text", PartialTextEvent { text });
            }
        });
    }
    if let Some(mut rx) = status_rx.take() {
        let app_status = app.clone();
        tokio::spawn(async move {
            while let Some(status) = rx.recv().await {
                let _ = app_status.emit(
                    "session://live-transcript-status",
                    LiveTranscriptStatusEvent {
                        state: status.state,
                        detail: status.detail,
                    },
                );
            }
        });
    }
    Ok(())
}

async fn active_session_mode_and_preset(state: &AppState) -> (Mode, Option<String>) {
    if let Some((mode, preset)) = {
        let session = state.session.lock().await;
        session
            .as_ref()
            .map(|controller| (controller.mode.clone(), controller.polish_preset.clone()))
    } {
        return (mode, Some(preset));
    }
    (state.mode.lock().await.clone(), None)
}

async fn stop_recording_locked(app: &tauri::AppHandle, state: &AppState) -> Result<String, String> {
    if !matches!(
        *state.recording_state.lock().await,
        RecordingState::Recording
    ) {
        return Ok(String::new());
    }

    let (mode, polish_preset) = active_session_mode_and_preset(state).await;
    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Processing,
            mode: mode.clone(),
            polish_preset: polish_preset.clone(),
            phase: "transcribing".to_string(),
            raw_text: None,
            final_text: None,
            history_id: None,
            error: None,
        },
    );

    let injector = TargetWindowInjector {
        target: state.last_target_window.lock().await.clone(),
    };
    let result = session_service::stop_session_inner(state, &injector, Some(app.clone())).await;
    *state.recording_trigger.lock().await = None;

    let has_error = match &result {
        Ok(outcome) => outcome.inject_error.is_some(),
        Err(_) => true,
    };
    let mode_label = if matches!(mode, Mode::Polish) {
        "Polish"
    } else {
        "Raw"
    };
    tray::update_status(
        app,
        if !has_error { "待機中" } else { "エラー" },
        mode_label,
    );
    let history_id = match &result {
        Ok(outcome) if !outcome.final_text.is_empty() && outcome.inject_error.is_none() => {
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
            if let (Some(recovery_id), Some(_)) = (&outcome.recovery_id, &saved_id) {
                if let Err(error) = recovery::delete_session(app, recovery_id) {
                    tracing::warn!("failed to delete completed recovery session: {error}");
                }
            } else if let Some(recovery_id) = &outcome.recovery_id {
                let _ =
                    recovery::mark_failed(app, recovery_id, "履歴保存に失敗しました。".to_string());
            }
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
        Ok(_) => None,
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
    };

    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Idle,
            mode,
            polish_preset,
            phase: if has_error { "failed" } else { "completed" }.to_string(),
            raw_text: result
                .as_ref()
                .ok()
                .map(|outcome| outcome.raw_text.clone())
                .filter(|text| !text.is_empty()),
            final_text: result
                .as_ref()
                .ok()
                .map(|outcome| outcome.final_text.clone())
                .filter(|text| !text.is_empty()),
            history_id,
            error: match &result {
                Ok(outcome) => outcome.inject_error.clone(),
                Err(error) => Some(error.clone()),
            },
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
    start_recording_locked(&app, &state, RecordingTrigger::Manual, None).await
}

#[tauri::command]
pub async fn stop_recording_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _guard = state.session_action.lock().await;
    stop_recording_locked(&app, &state).await
}

async fn stop_recording_preview_locked(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    if !matches!(
        *state.recording_state.lock().await,
        RecordingState::Recording
    ) {
        return Ok(String::new());
    }

    let (mode, polish_preset) = active_session_mode_and_preset(state).await;
    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Processing,
            mode: mode.clone(),
            polish_preset: polish_preset.clone(),
            phase: "transcribing".to_string(),
            raw_text: None,
            final_text: None,
            history_id: None,
            error: None,
        },
    );

    let result =
        session_service::stop_session_inner(state, &PreviewInjector, Some(app.clone())).await;
    *state.recording_trigger.lock().await = None;

    let has_error = result.is_err();
    let mode_label = if matches!(mode, Mode::Polish) {
        "Polish"
    } else {
        "Raw"
    };
    tray::update_status(
        app,
        if !has_error { "待機中" } else { "エラー" },
        mode_label,
    );

    if let Ok(outcome) = &result {
        if let Some(recovery_id) = &outcome.recovery_id {
            if let Err(error) = recovery::delete_session(app, recovery_id) {
                tracing::warn!("failed to delete onboarding test recovery session: {error}");
            }
        }
    }

    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Idle,
            mode,
            polish_preset,
            phase: if has_error { "failed" } else { "completed" }.to_string(),
            raw_text: result
                .as_ref()
                .ok()
                .map(|outcome| outcome.raw_text.clone())
                .filter(|text| !text.is_empty()),
            final_text: result
                .as_ref()
                .ok()
                .map(|outcome| outcome.final_text.clone())
                .filter(|text| !text.is_empty()),
            history_id: None,
            error: result.as_ref().err().cloned(),
        },
    );
    result.map(|outcome| outcome.final_text)
}

#[tauri::command]
pub async fn start_onboarding_test_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.session_action.lock().await;
    start_recording_locked(&app, &state, RecordingTrigger::Manual, None).await
}

#[tauri::command]
pub async fn stop_onboarding_test_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _guard = state.session_action.lock().await;
    stop_recording_preview_locked(&app, &state).await
}

#[tauri::command]
pub async fn push_to_talk_down(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.session_action.lock().await;
    let recording_state = state.recording_state.lock().await.clone();
    match push_to_talk_down_action(&recording_state) {
        ShortcutAction::Start(trigger) => start_recording_locked(&app, &state, trigger, None).await,
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
    toggle_hands_free_recording_for_mode(app, state, Mode::Raw).await
}

#[tauri::command]
pub async fn toggle_hands_free_recording_for_mode(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mode: Mode,
) -> Result<(), String> {
    let _guard = state.session_action.lock().await;
    let recording_state = state.recording_state.lock().await.clone();
    match hands_free_action(&recording_state) {
        ShortcutAction::Start(trigger) => {
            start_recording_locked(&app, &state, trigger, Some(mode)).await
        }
        ShortcutAction::Stop => {
            stop_recording_locked(&app, &state).await?;
            Ok(())
        }
        ShortcutAction::Ignore => Ok(()),
    }
}

fn normalize_polish_preset(preset: &str) -> Result<String, String> {
    match preset.trim() {
        "slack" | "email" | "memo" | "prompt" | "technical" => Ok(preset.trim().to_string()),
        _ => Err("未対応のPolishプリセットです。".to_string()),
    }
}

#[tauri::command]
pub async fn set_active_polish_preset(
    state: State<'_, AppState>,
    preset: String,
) -> Result<String, String> {
    let preset = normalize_polish_preset(&preset)?;
    let mut session = state.session.lock().await;
    let Some(controller) = session.as_mut() else {
        return Err("録音中のPolishセッションがありません。".to_string());
    };
    if !matches!(controller.mode, Mode::Polish) {
        return Err("Raw録音中はPolishプリセットを変更できません。".to_string());
    }
    controller.polish_preset = preset.clone();
    Ok(preset)
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
pub async fn inject_text(state: State<'_, AppState>, text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("注入するテキストがありません。".to_string());
    }
    let target = state
        .last_target_window
        .lock()
        .await
        .clone()
        .ok_or_else(|| "入力先アプリを一度クリックしてから再注入してください。".to_string())?;
    crate::inject::inject_text_to_window(&text, &target).map_err(|error| error.to_string())
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
pub async fn toggle_history_pin(
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<HistoryEntry>, String> {
    local_data::toggle_history_pin(&app, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn rerun_history_polish(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<HistoryEntry>, String> {
    let mut history = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let index = history
        .iter()
        .position(|item| item.id == id)
        .ok_or_else(|| "履歴が見つかりません。".to_string())?;
    let source_text = if !history[index].raw_text.trim().is_empty() {
        history[index].raw_text.clone()
    } else {
        history[index].final_text.clone()
    };
    if source_text.trim().is_empty() {
        return Err("Polish再実行に使えるテキストがありません。".to_string());
    }

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
    let polished = mode::route(
        &Mode::Polish,
        &current_settings,
        &dictionary_words,
        focused_context.as_ref(),
        &source_text,
    )
    .await;
    let snippets = local_data::load_snippets(&app).map_err(|error| error.to_string())?;
    let final_text = local_data::expand_snippets(&polished, &snippets);
    history[index].raw_text = source_text;
    history[index].final_text = final_text;
    history[index].mode = Mode::Polish;
    history[index].status = local_data::HistoryStatus::Success;
    history[index].error = None;
    local_data::save_history(&app, &history).map_err(|error| error.to_string())?;
    Ok(history)
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
pub async fn get_dictionary_suggestions(
    app: tauri::AppHandle,
) -> Result<Vec<DictionarySuggestion>, String> {
    local_data::dictionary_suggestions(&app).map_err(|error| error.to_string())
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
pub async fn get_snippets(app: tauri::AppHandle) -> Result<Vec<SnippetEntry>, String> {
    local_data::load_snippets(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn add_snippet(
    app: tauri::AppHandle,
    cue: String,
    text: String,
) -> Result<Vec<SnippetEntry>, String> {
    local_data::add_snippet(&app, &cue, &text).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn remove_snippet(
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<SnippetEntry>, String> {
    local_data::remove_snippet(&app, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_usage_summary(app: tauri::AppHandle) -> Result<Vec<UsageDaySummary>, String> {
    local_data::load_usage(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_recovery_sessions(
    app: tauri::AppHandle,
) -> Result<Vec<RecoverySessionSummary>, String> {
    recovery::list_sessions(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn retry_recovery_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<RecoverySessionSummary, String> {
    let meta = recovery::load_meta(&app, &id).map_err(|error| error.to_string())?;
    let wav_path = recovery::audio_path(&app, &id).map_err(|error| error.to_string())?;
    if !wav_path.exists() {
        return Err("復元できる音声ファイルがありません。".to_string());
    }
    let wav_info = recovery::repair_wav_header(&wav_path).map_err(|error| error.to_string())?;
    recovery::mark_captured(
        &app,
        &id,
        wav_info.sample_rate,
        wav_info.channels,
        wav_info.frame_count,
        wav_info.duration_ms,
    )
    .map_err(|error| error.to_string())?;

    let current_settings = state.settings.lock().await.clone();
    if current_settings.api_key.is_empty() {
        let error = "APIキーが設定されていません。設定画面から入力してください。".to_string();
        let _ = recovery::mark_failed(&app, &id, error.clone());
        return Err(error);
    }
    let dictionary_words = state.dictionary_words.lock().await.clone();
    let focused_context = if current_settings.deep_context_enabled {
        context::focused_app_context()
    } else {
        None
    };
    recovery::mark_transcribing(&app, &id).map_err(|error| error.to_string())?;

    let audio = audio::CapturedAudio {
        samples: Vec::new(),
        sample_rate: wav_info.sample_rate,
        channels: wav_info.channels,
        wav_path: Some(wav_path),
        frame_count: wav_info.frame_count,
    };
    let provider = OpenAiCompatibleProvider {
        base_url: current_settings.api_base_url.clone(),
        api_key: current_settings.api_key.clone(),
        model: current_settings.api_model.clone(),
        dictionary_words: dictionary_words.clone(),
        focused_context: focused_context.clone(),
        partial_tx: None,
    };
    let raw_text = match provider.transcribe(&audio).await {
        Ok(text) => text,
        Err(error) => {
            let error = error.to_string();
            let _ = recovery::mark_failed(&app, &id, error.clone());
            return Err(error);
        }
    };
    let final_text = mode::route(
        &meta.mode,
        &current_settings,
        &dictionary_words,
        focused_context.as_ref(),
        &raw_text,
    )
    .await;
    let snippets = local_data::load_snippets(&app).map_err(|error| error.to_string())?;
    let final_text = local_data::expand_snippets(&final_text, &snippets);
    recovery::mark_text_ready(&app, &id, raw_text, final_text)
        .map_err(|error| error.to_string())?;
    recovery::summarize(&app, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn inject_recovery_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<RecoverySessionSummary, String> {
    let meta = recovery::load_meta(&app, &id).map_err(|error| error.to_string())?;
    if meta.final_text.trim().is_empty() {
        return Err("再注入できるテキストがありません。".to_string());
    }
    let target = state
        .last_target_window
        .lock()
        .await
        .clone()
        .ok_or_else(|| "入力先アプリを一度クリックしてから再注入してください。".to_string())?;
    if let Err(error) = crate::inject::inject_text_to_window(&meta.final_text, &target) {
        let error = error.to_string();
        let _ = recovery::mark_failed(&app, &id, error.clone());
        return Err(error);
    }
    recovery::update_meta(&app, &id, |meta| {
        meta.status = recovery::RecoveryStatus::TextReady;
        meta.error = None;
    })
    .map_err(|error| error.to_string())?;
    recovery::summarize(&app, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_recovery_session_to_history(
    app: tauri::AppHandle,
    id: String,
) -> Result<HistoryEntry, String> {
    let meta = recovery::load_meta(&app, &id).map_err(|error| error.to_string())?;
    if meta.final_text.trim().is_empty() {
        return Err("履歴に保存できるテキストがありません。".to_string());
    }
    let entry = local_data::make_history_entry(
        meta.raw_text.clone(),
        meta.final_text.clone(),
        meta.mode.clone(),
        meta.duration_ms,
        None,
    );
    let saved = local_data::append_history(&app, entry).map_err(|error| error.to_string())?;
    recovery::mark_completed(&app, &id, Some(saved.id.clone()))
        .map_err(|error| error.to_string())?;
    recovery::delete_session(&app, &id).map_err(|error| error.to_string())?;
    Ok(saved)
}

#[tauri::command]
pub async fn delete_recovery_session(
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<RecoverySessionSummary>, String> {
    recovery::delete_session(&app, &id).map_err(|error| error.to_string())?;
    recovery::list_sessions(&app).map_err(|error| error.to_string())
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
pub async fn snippet_limit() -> Result<usize, String> {
    Ok(MAX_SNIPPETS)
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
    new_settings.hands_free_raw_hotkey = normalized.hands_free_raw;
    new_settings.hands_free_polish_hotkey = normalized.hands_free_polish;
    if previous.launch_at_login != new_settings.launch_at_login {
        if let Err(error) = crate::startup::set_launch_at_login(new_settings.launch_at_login) {
            let _ = hotkey::reconfigure_hotkeys(hotkey_set(&previous));
            return Err(format!(
                "ログイン時起動の設定を変更できませんでした: {error}"
            ));
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
        read_api_key_from_env_file, select_api_key, settings_for_ui, should_start_realtime_asr,
        ShortcutAction,
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
            hands_free_raw_hotkey: HotkeyBinding::hands_free_raw_default(),
            hands_free_polish_hotkey: HotkeyBinding::hands_free_polish_default(),
            mode: Mode::Raw,
            ..Default::default()
        };
        let payload = settings_for_ui(&settings).unwrap();
        assert_eq!(payload.get("api_key").unwrap(), "");
        assert_eq!(payload.get("has_api_key").unwrap(), true);
        assert!(!payload.to_string().contains("sk-test-secret"));
    }

    #[test]
    fn raw_live_transcript_off_skips_realtime_asr() {
        let settings = AppSettings {
            api_key: "sk-test".to_string(),
            show_live_transcript_in_floating_bar: false,
            ..Default::default()
        };

        assert!(!should_start_realtime_asr(&settings, &Mode::Raw));
    }

    #[test]
    fn raw_live_transcript_on_uses_realtime_asr() {
        let settings = AppSettings {
            api_key: "sk-test".to_string(),
            show_live_transcript_in_floating_bar: true,
            ..Default::default()
        };

        assert!(should_start_realtime_asr(&settings, &Mode::Raw));
    }

    #[test]
    fn polish_live_transcript_off_keeps_realtime_asr() {
        let settings = AppSettings {
            api_key: "sk-test".to_string(),
            show_live_transcript_in_floating_bar: false,
            ..Default::default()
        };

        assert!(should_start_realtime_asr(&settings, &Mode::Polish));
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
