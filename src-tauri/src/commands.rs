use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tauri::{Emitter, Manager, State};

use crate::{
    app_profiles::{self, AppProfile, AppProfileInput, EffectiveAppProfile, ProfileMutationResult},
    audio,
    context::{self, FocusedAppContext, FocusedWindowTarget},
    corrections::{
        self, ComparisonMode, CorrectionArtifact, CorrectionCandidatePersistenceState,
        CorrectionPreview, CorrectionRecord, CorrectionStatus, NewCorrection, UpdateCorrection,
        VocabularyCandidateAssociation,
    },
    data_flow,
    hotkey::{self, HotkeySet},
    local_data::{
        self, DictionarySuggestion, HistoryEntry, OperationKind, SessionMetrics, SnippetEntry,
        UsageDaySummary, MAX_DICTIONARY_WORDS, MAX_SNIPPETS,
    },
    mode,
    polish::PolishState,
    recovery::{self, RecoverySessionSummary},
    selected_learning::{
        self, ConfirmedSelectedPreview, PrepareSelectedCorrectionResult, ResolvedPendingSelection,
        SelectedCorrectionOperation, SelectedLearningReplay,
    },
    selected_voice_edit::{
        self, ActiveSelectedVoiceEdit, ReplaceDecision, SelectedVoiceEditPreview,
    },
    session_service,
    settings::{self, AppSettings, CorrectionLearningMode},
    speech::{
        openai_compatible::OpenAiCompatibleProvider,
        realtime::{realtime_vocabulary_prompt, transcribe_realtime, RealtimeStatus},
        SpeechProvider,
    },
    state::{AppState, Mode, ProcessingGateOutcome, RecordingState, RecordingTrigger, SessionKind},
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
    polish_state: Option<PolishState>,
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

#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SelectedVoiceEditToggleResult {
    Recording { warning: Option<String> },
    Preview { preview: SelectedVoiceEditPreview },
}

#[derive(serde::Serialize)]
pub struct SelectedVoiceEditReplaceResult {
    pub replaced: bool,
    pub code: String,
    pub message: String,
    pub partial: bool,
}

#[derive(serde::Serialize)]
pub struct CreateSelectedCorrectionResult {
    pub record_id: String,
    pub replayed: bool,
    pub focus_warning: Option<String>,
}

fn selected_focus_warning(result: anyhow::Result<()>) -> Option<String> {
    result
        .err()
        .map(|_| "修正内容は保存しましたが、元の入力先へフォーカスを戻せませんでした。".to_string())
}

trait UserInjectionBackend {
    fn inject_current(&self, text: &str) -> anyhow::Result<crate::inject::InjectionSuccess>;
    fn inject_target(
        &self,
        text: &str,
        target: &FocusedWindowTarget,
    ) -> anyhow::Result<crate::inject::InjectionSuccess>;
}

struct SystemUserInjectionBackend;

impl UserInjectionBackend for SystemUserInjectionBackend {
    fn inject_current(&self, text: &str) -> anyhow::Result<crate::inject::InjectionSuccess> {
        crate::inject::inject_text(text)
    }

    fn inject_target(
        &self,
        text: &str,
        target: &FocusedWindowTarget,
    ) -> anyhow::Result<crate::inject::InjectionSuccess> {
        crate::inject::inject_text_to_window(text, target)
    }
}

fn dispatch_user_injection(
    backend: &impl UserInjectionBackend,
    text: &str,
    target: Option<&FocusedWindowTarget>,
) -> anyhow::Result<crate::inject::InjectionSuccess> {
    match target {
        Some(target) => backend.inject_target(text, target),
        None => backend.inject_current(text),
    }
}

struct TargetWindowInjector {
    target: Option<FocusedWindowTarget>,
}

impl session_service::TextInjector for TargetWindowInjector {
    fn inject(&self, text: &str) -> anyhow::Result<crate::inject::InjectionSuccess> {
        dispatch_user_injection(&SystemUserInjectionBackend, text, self.target.as_ref())
    }
}

struct PreviewInjector;

impl session_service::TextInjector for PreviewInjector {
    fn inject(&self, _text: &str) -> anyhow::Result<crate::inject::InjectionSuccess> {
        Ok(crate::inject::InjectionSuccess { warning: None })
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
const FLOATING_BAR_WIDTH: f64 = 380.0;
const FLOATING_BAR_BOTTOM_GAP: f64 = 16.0;

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
        settings.learn_selected_hotkey.clone(),
        settings.voice_edit_selected_hotkey.clone(),
    )
}

async fn append_history_locked(
    state: &AppState,
    app: &tauri::AppHandle,
    entry: HistoryEntry,
) -> anyhow::Result<HistoryEntry> {
    let _guard = state.history_action.lock().await;
    local_data::append_history(app, entry)
}

fn settings_rollback_error(
    save_error: &str,
    hotkey_error: Option<String>,
    startup_error: Option<String>,
) -> String {
    let mut message = format!("設定を保存できませんでした: {save_error}");
    if let Some(error) = hotkey_error {
        message.push_str(&format!(
            "。以前のショートカットの復元にも失敗しました: {error}"
        ));
    }
    if let Some(error) = startup_error {
        message.push_str(&format!(
            "。以前のログイン時起動設定の復元にも失敗しました: {error}"
        ));
    }
    message
}

fn startup_change_error(startup_error: &str, hotkey_restore_error: Option<String>) -> String {
    match hotkey_restore_error {
        Some(restore_error) => format!(
            "ログイン時起動の設定を変更できませんでした: {startup_error}。以前のショートカットの復元にも失敗しました: {restore_error}"
        ),
        None => format!("ログイン時起動の設定を変更できませんでした: {startup_error}"),
    }
}

#[cfg(test)]
fn should_start_realtime_asr(settings: &AppSettings, session_mode: &Mode) -> bool {
    data_flow::effective_realtime_model(settings, session_mode).is_some()
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

#[tauri::command]
pub async fn resize_and_position_floating_bar(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    height: f64,
) -> Result<(), String> {
    let window = app
        .get_webview_window("floating-bar")
        .ok_or_else(|| "floating-bar window not found".to_string())?;

    window
        .set_size(tauri::LogicalSize::new(FLOATING_BAR_WIDTH, height))
        .map_err(|error| error.to_string())?;

    let window_size = window.outer_size().map_err(|error| error.to_string())?;
    let scale = if height > 0.0 {
        f64::from(window_size.height) / height
    } else {
        window.scale_factor().map_err(|error| error.to_string())?
    };
    let bottom_gap = (FLOATING_BAR_BOTTOM_GAP * scale).round() as i32;
    let target = state.last_target_window.lock().await.clone();
    let work_area = target
        .as_ref()
        .and_then(context::window_work_area)
        .or_else(context::primary_work_area)
        .ok_or_else(|| "target monitor work area not found".to_string())?;

    let window_width = window_size.width as i32;
    let window_height = window_size.height as i32;
    let x = work_area.x + (work_area.width - window_width).max(0) / 2;
    let y = work_area.y + work_area.height - window_height - bottom_gap;

    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

async fn start_recording_locked(
    app: &tauri::AppHandle,
    state: &AppState,
    trigger: RecordingTrigger,
    mode_override: Option<Mode>,
) -> Result<(), String> {
    if state.pending_selected_learning.lock().await.is_some()
        || state.pending_selected_voice_edit.lock().await.is_some()
        || state.active_selected_voice_edit.lock().await.is_some()
    {
        return Err(
            "別の選択テキスト操作が進行中です。先に完了またはキャンセルしてください。".to_string(),
        );
    }
    if !matches!(*state.recording_state.lock().await, RecordingState::Idle) {
        return Ok(());
    }

    let focused_target = context::current_external_focused_window();
    if let Some(target) = &focused_target {
        *state.last_target_window.lock().await = Some(target.clone());
    }

    let (level_tx, mut level_rx) = tokio::sync::mpsc::unbounded_channel::<f32>();
    let settings = state.settings.lock().await.clone();
    let focused_context_for_preset = focused_target.as_ref().map(|target| FocusedAppContext {
        process_name: target.process_name.clone(),
        window_title: target.window_title.clone(),
    });
    let profile_store = {
        let _guard = state.app_profiles_action.lock().await;
        app_profiles::load(app).map_err(|error| error.to_string())?
    };
    let effective_profile = app_profiles::resolve(
        &profile_store,
        focused_context_for_preset.as_ref(),
        &settings,
        mode_override,
    );
    let session_mode = effective_profile.mode.clone();
    let session_polish_preset = effective_profile.polish_preset.clone();
    let session_app_process = focused_target
        .as_ref()
        .map(|target| target.process_name.clone())
        .unwrap_or_default();
    let manual_dictionary = state.dictionary_words.lock().await.clone();
    let correction_snapshot = if settings.correction_learning_mode == CorrectionLearningMode::Ask {
        let _guard = state.corrections_action.lock().await;
        let store = corrections::load(app).map_err(|error| error.to_string())?;
        corrections::make_session_snapshot(
            CorrectionLearningMode::Ask,
            &manual_dictionary,
            store,
            &session_app_process,
        )
    } else {
        corrections::make_session_snapshot(
            CorrectionLearningMode::Off,
            &manual_dictionary,
            corrections::CorrectionStore::default(),
            &session_app_process,
        )
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
    let realtime_model = data_flow::effective_realtime_model(&settings, &session_mode);
    let (chunk_tx, realtime_task) = if let Some(realtime_model) = realtime_model {
        tracing::info!(
            configured_model = %settings.api_model,
            realtime_model = %realtime_model,
            live_transcript = settings.show_live_transcript_in_floating_bar,
            mode = ?session_mode,
            "starting recording with realtime ASR"
        );
        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::unbounded_channel();
        let realtime_prompt = realtime_vocabulary_prompt(
            &settings.api_base_url,
            &realtime_model,
            &correction_snapshot.dictionary_words,
        );
        let task = tokio::spawn(transcribe_realtime(
            settings.api_base_url.clone(),
            settings.api_key.clone(),
            realtime_model,
            effective_profile.language_mode,
            realtime_prompt,
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
    let recovery_session = recovery::create_session(
        app,
        session_mode.clone(),
        trigger_label(&trigger),
        session_polish_preset.clone(),
        session_app_process.clone(),
    )
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
        session_app_process,
        correction_snapshot,
        effective_profile.language_mode,
        focused_target,
    )
    .await
    {
        let _ = recovery::delete_session(app, &recovery_session.id);
        return Err(error);
    }
    *state.session_kind.lock().await = Some(SessionKind::Dictation);
    let cancel_hotkey_error = hotkey::set_cancel_hotkey_enabled(true).err().map(|error| {
        tracing::warn!("recording started without Escape cancellation: {error}");
        format!("録音は開始しましたが、Escapeによるキャンセルを有効にできませんでした: {error}")
    });
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
            polish_state: None,
            error: cancel_hotkey_error,
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

async fn active_session_snapshot(
    state: &AppState,
) -> (Mode, Option<String>, Option<FocusedWindowTarget>) {
    if let Some((mode, preset, target)) = {
        let session = state.session.lock().await;
        session.as_ref().map(|controller| {
            (
                controller.mode.clone(),
                controller.polish_preset.clone(),
                controller.target_window.clone(),
            )
        })
    } {
        return (mode, Some(preset), target);
    }
    (
        state.mode.lock().await.clone(),
        None,
        state.last_target_window.lock().await.clone(),
    )
}

async fn stop_recording_locked(app: &tauri::AppHandle, state: &AppState) -> Result<String, String> {
    if *state.session_kind.lock().await == Some(SessionKind::SelectedVoiceEdit) {
        return Err("選択音声編集は専用ホットキーでもう一度停止してください。".to_string());
    }
    let recording_state = state.recording_state.lock().await.clone();
    if !matches!(recording_state, RecordingState::Recording) {
        return Ok(String::new());
    }
    match state.commit_processing_or_observe_cancel() {
        ProcessingGateOutcome::CancelRequested => {
            cancel_recording_locked(app, state).await?;
            return Ok(String::new());
        }
        ProcessingGateOutcome::ProcessingCommitted => {}
    }
    // 通常停止のcommit point。以降に検出されたEscapeでは処理を巻き戻さない。
    *state.recording_state.lock().await = RecordingState::Processing;

    if let Err(error) = hotkey::set_cancel_hotkey_enabled(false) {
        tracing::warn!("failed to disable recording cancel hotkey before stop: {error}");
    }

    let (mode, polish_preset, target_window) = active_session_snapshot(state).await;
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
            polish_state: None,
            error: None,
        },
    );

    let injector = TargetWindowInjector {
        target: target_window,
    };
    let result = session_service::stop_session_inner(state, &injector, Some(app.clone())).await;
    *state.session_kind.lock().await = None;
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
            let entry = local_data::make_history_entry_with_context(
                outcome.raw_text.clone(),
                outcome.final_text.clone(),
                outcome.mode.clone(),
                outcome.duration_ms,
                None,
                outcome.polish_state.clone(),
                outcome.polish_preset.clone(),
                outcome.app_process.clone(),
            );
            let saved_id = append_history_locked(state, app, entry)
                .await
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
                    polish_model: outcome
                        .polish_model_used
                        .clone()
                        .unwrap_or(settings.polish_model),
                    polish_usage: outcome.polish_usage,
                },
            ) {
                tracing::warn!("failed to record usage: {error}");
            }
            saved_id
        }
        Ok(outcome) => {
            if let (Some(usage), Some(model)) =
                (outcome.polish_usage, outcome.polish_model_used.as_deref())
            {
                if let Err(error) = local_data::record_polish_usage_only(app, model, usage) {
                    tracing::warn!("failed to record incomplete session Polish usage: {error}");
                }
            }
            None
        }
        Err(error) => {
            let entry = local_data::make_history_entry(
                String::new(),
                String::new(),
                mode.clone(),
                0,
                Some(error.clone()),
                PolishState::Unknown,
            );
            append_history_locked(state, app, entry)
                .await
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
            polish_state: result
                .as_ref()
                .ok()
                .map(|outcome| outcome.polish_state.clone()),
            error: match &result {
                Ok(outcome) => outcome
                    .inject_error
                    .clone()
                    .or_else(|| outcome.inject_warning.clone()),
                Err(error) => Some(error.clone()),
            },
        },
    );
    result.map(|outcome| outcome.final_text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingCancelRoute {
    Dictation,
    SelectedVoiceEdit,
}

fn recording_cancel_route(kind: Option<SessionKind>) -> RecordingCancelRoute {
    if kind == Some(SessionKind::SelectedVoiceEdit) {
        RecordingCancelRoute::SelectedVoiceEdit
    } else {
        RecordingCancelRoute::Dictation
    }
}

fn complete_dictation_cancel(
    outcome: &session_service::CancelSessionOutcome,
    mut cleanup_recovery: impl FnMut(&str) -> anyhow::Result<()>,
) -> Result<bool, String> {
    if !outcome.cancelled {
        return Ok(false);
    }
    if let Some(id) = outcome.recovery_id.as_deref() {
        cleanup_recovery(id).map_err(|error| {
            format!("キャンセルした録音の一時データを削除できませんでした: {error}")
        })?;
    }
    Ok(true)
}

async fn cancel_recording_locked(app: &tauri::AppHandle, state: &AppState) -> Result<bool, String> {
    if recording_cancel_route(*state.session_kind.lock().await)
        == RecordingCancelRoute::SelectedVoiceEdit
    {
        return cancel_selected_voice_edit_recording_locked(app, state).await;
    }
    if !matches!(
        *state.recording_state.lock().await,
        RecordingState::Recording
    ) {
        return Ok(false);
    }

    let (mode, polish_preset, _) = active_session_snapshot(state).await;
    if let Err(error) = hotkey::set_cancel_hotkey_enabled(false) {
        tracing::warn!("failed to disable recording cancel hotkey during cancellation: {error}");
    }
    let outcome = session_service::cancel_session_inner(state).await;
    *state.session_kind.lock().await = None;
    if !outcome.cancelled {
        return Ok(false);
    }
    if let Some(error) = &outcome.capture_error {
        tracing::warn!("audio capture ended with an error during cancellation: {error}");
    }

    let completion =
        complete_dictation_cancel(&outcome, |id| recovery::cleanup_completed_session(app, id));
    let cleanup_error = completion.as_ref().err().cloned();
    let mode_label = if matches!(mode, Mode::Polish) {
        "Polish"
    } else {
        "Raw"
    };
    tray::update_status(app, "待機中", mode_label);
    let _ = app.emit(
        "session://state-changed",
        SessionUiEvent {
            state: RecordingState::Idle,
            mode,
            polish_preset,
            phase: "cancelled".to_string(),
            raw_text: None,
            final_text: None,
            history_id: None,
            polish_state: None,
            error: cleanup_error.clone(),
        },
    );

    completion
}

async fn start_selected_voice_edit_locked(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<SelectedVoiceEditToggleResult, String> {
    let recording_state = state.recording_state.lock().await.clone();
    let has_session = state.session.lock().await.is_some();
    let has_selected_learning = state.pending_selected_learning.lock().await.is_some();
    let has_active = state.active_selected_voice_edit.lock().await.is_some();
    let has_pending = state.pending_selected_voice_edit.lock().await.is_some();
    selected_voice_edit::ensure_operation_available(
        &recording_state,
        has_session,
        has_selected_learning,
        has_active,
        has_pending,
    )?;
    let settings = state.settings.lock().await.clone();
    if settings.api_key.trim().is_empty() {
        return Err("APIキーが設定されていません。設定画面から入力してください。".to_string());
    }
    if settings.polish_model.trim().is_empty() {
        return Err("選択音声編集に使うPolishモデルが設定されていません。".to_string());
    }

    // KoeTypeへfocusを移す前に対象windowと選択本文をmemory snapshotする。
    let capture = crate::selection::capture_selected_text()
        .await
        .map_err(|error| error.to_string())?;
    let recovery_session = recovery::create_session_with_operation(
        app,
        Mode::Polish,
        "selected_voice_edit".to_string(),
        "selected_voice_edit".to_string(),
        capture.target.process_name.clone(),
        OperationKind::SelectedVoiceEdit,
    )
    .map_err(|error| error.to_string())?;
    if let Err(error) = session_service::start_session_inner(
        state,
        None,
        None,
        None,
        Some(recovery_session.id.clone()),
        Some(recovery_session.audio_path.clone()),
        Mode::Polish,
        "selected_voice_edit".to_string(),
        capture.target.process_name.clone(),
        corrections::CorrectionSessionSnapshot::default(),
        settings.language_mode,
        Some(capture.target.clone()),
    )
    .await
    {
        let _ = recovery::delete_session(app, &recovery_session.id);
        return Err(error);
    }
    *state.active_selected_voice_edit.lock().await = Some(ActiveSelectedVoiceEdit {
        target: capture.target,
        original_text: capture.text,
        selection_method: capture.method,
        selection_warning: capture.warning,
        recovery_id: recovery_session.id,
    });
    *state.session_kind.lock().await = Some(SessionKind::SelectedVoiceEdit);
    *state.recording_trigger.lock().await = Some(RecordingTrigger::Manual);
    let warning = hotkey::set_cancel_hotkey_enabled(true).err().map(|error| {
        format!("録音は開始しましたが、Escapeキャンセルを登録できませんでした: {error}")
    });
    if let Err(error) = show_and_focus_main(app) {
        let _ = cancel_selected_voice_edit_recording_locked(app, state).await;
        return Err(error);
    }
    tray::update_status(app, "選択編集を録音中", "Voice Edit");
    Ok(SelectedVoiceEditToggleResult::Recording { warning })
}

async fn stop_selected_voice_edit_locked(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<SelectedVoiceEditPreview, String> {
    if !matches!(
        *state.recording_state.lock().await,
        RecordingState::Recording
    ) {
        return Err("選択音声編集は録音中ではありません。".to_string());
    }
    match state.commit_processing_or_observe_cancel() {
        ProcessingGateOutcome::CancelRequested => {
            cancel_selected_voice_edit_recording_locked(app, state).await?;
            return Err("選択音声編集をキャンセルしました。".to_string());
        }
        ProcessingGateOutcome::ProcessingCommitted => {}
    }
    *state.recording_state.lock().await = RecordingState::Processing;
    if let Err(error) = hotkey::set_cancel_hotkey_enabled(false) {
        tracing::warn!("failed to disable selected voice edit Escape hotkey: {error}");
    }
    let controller = state.session.lock().await.take();
    let active = state.active_selected_voice_edit.lock().await.take();
    let (controller, active) = match (controller, active) {
        (Some(controller), Some(active)) => (controller, active),
        (controller, active) => {
            if let Some(controller) = controller {
                let _ = controller.stop_tx.send(true);
                let _ = controller.capture_task.await;
            }
            if let Some(active) = active {
                let _ = recovery::mark_failed(
                    app,
                    &active.recovery_id,
                    "選択音声編集の内部状態が不整合でした。元の選択本文は変更していません。"
                        .to_string(),
                );
            }
            *state.recording_state.lock().await = RecordingState::Idle;
            *state.recording_trigger.lock().await = None;
            *state.session_kind.lock().await = None;
            state.reset_cancellation_gate();
            tray::update_status(app, "エラー", "Voice Edit");
            return Err(
                "選択音声編集の内部状態が不整合でした。元の選択本文は変更していません。"
                    .to_string(),
            );
        }
    };
    let duration_ms = controller.started_at.elapsed().as_millis() as u64;

    let result: Result<SelectedVoiceEditPreview, String> = async {
        let _ = controller.stop_tx.send(true);
        let audio = controller
            .capture_task
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        recovery::mark_captured(
            app,
            &active.recovery_id,
            audio.sample_rate,
            audio.channels,
            audio.frames(),
            duration_ms,
        )
        .map_err(|error| error.to_string())?;
        recovery::mark_transcribing(app, &active.recovery_id)
            .map_err(|error| error.to_string())?;
        let settings = state.settings.lock().await.clone();
        let provider = OpenAiCompatibleProvider {
            base_url: settings.api_base_url.clone(),
            api_key: settings.api_key.clone(),
            model: settings.api_model.clone(),
            language_mode: settings.language_mode,
            dictionary_words: Vec::new(),
            focused_context: None,
            partial_tx: None,
        };
        let instruction = match tokio::time::timeout(
            std::time::Duration::from_secs(selected_voice_edit::ASR_TIMEOUT_SECS),
            provider.transcribe(&audio),
        )
        .await
        {
            Ok(result) => result.map_err(|error| {
                let message = error.to_string();
                let _ = recovery::mark_failed(app, &active.recovery_id, message.clone());
                message
            })?,
            Err(_) => {
                let message = "選択音声編集の音声認識が90秒でタイムアウトしました。元の選択本文は変更していません。".to_string();
                let _ = recovery::mark_failed(app, &active.recovery_id, message.clone());
                return Err(message);
            }
        };
        if instruction.trim().is_empty() {
            let message = "音声編集指示を認識できませんでした。元の選択本文は変更していません。".to_string();
            let _ = recovery::mark_failed(app, &active.recovery_id, message.clone());
            return Err(message);
        }
        // 原選択本文はrecoveryへ保存しない。raw_text相当は音声指示だけ。
        recovery::mark_text_ready(app, &active.recovery_id, instruction.clone(), String::new())
            .map_err(|error| error.to_string())?;
        let proposal_attempt = match tokio::time::timeout(
            std::time::Duration::from_secs(selected_voice_edit::EDIT_TIMEOUT_SECS),
            crate::polish::edit_selected_text_attempt(&settings, &active.original_text, &instruction),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                let message = "選択音声編集APIが90秒でタイムアウトしました。元の選択本文は変更していません。".to_string();
                let _ = recovery::mark_failed(app, &active.recovery_id, message.clone());
                return Err(message);
            }
        };
        if let Some(usage) = proposal_attempt.usage {
            if let Err(error) = local_data::record_polish_usage_only(app, &proposal_attempt.model_used, usage) {
                tracing::warn!("failed to record selected voice edit usage: {error}");
            }
        }
        let proposal = proposal_attempt.result.map_err(|error| {
            let message = format!("選択音声編集APIに失敗しました。元の選択本文は変更していません: {error}");
            let _ = recovery::mark_failed(app, &active.recovery_id, message.clone());
            message
        })?;
        if proposal.trim().is_empty()
            || proposal.chars().count() > crate::selection::MAX_SELECTED_TEXT_CHARS
        {
            let message = "選択音声編集APIの提案本文が空、または20,000文字を超えています。元の選択本文は変更していません。".to_string();
            let _ = recovery::mark_failed(app, &active.recovery_id, message.clone());
            return Err(message);
        }
        recovery::mark_text_ready(
            app,
            &active.recovery_id,
            instruction.clone(),
            proposal.clone(),
        )
        .map_err(|error| error.to_string())?;

        let history = local_data::make_selected_voice_edit_history_entry(
            instruction.clone(),
            proposal.clone(),
            duration_ms,
            None,
            active.target.process_name.clone(),
        );
        let history_id = match append_history_locked(state, app, history).await {
            Ok(saved) => {
                if let Err(error) = recovery::delete_session(app, &active.recovery_id) {
                    tracing::warn!("failed to delete completed selected voice edit recovery: {error}");
                }
                Some(saved.id)
            }
            Err(error) => {
                tracing::warn!("failed to save selected voice edit history: {error}");
                let _ = recovery::mark_failed(
                    app,
                    &active.recovery_id,
                    "選択音声編集の履歴保存に失敗しました。previewから提案を回収してください。".to_string(),
                );
                None
            }
        };
        let preview = {
            let mut pending = state.pending_selected_voice_edit.lock().await;
            selected_voice_edit::replace_pending(
                &mut pending,
                active,
                instruction,
                proposal,
                history_id,
                selected_voice_edit::now_secs(),
            )
        };
        let expiry_app = app.clone();
        let expiry_token = preview.token.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(
                selected_voice_edit::PREVIEW_TTL_SECS,
            ))
            .await;
            let managed = expiry_app.state::<AppState>();
            let mut pending = managed.pending_selected_voice_edit.lock().await;
            selected_voice_edit::expire_pending(&mut pending, &expiry_token);
        });
        Ok(preview)
    }
    .await;

    *state.recording_state.lock().await = RecordingState::Idle;
    *state.recording_trigger.lock().await = None;
    *state.session_kind.lock().await = None;
    state.reset_cancellation_gate();
    tray::update_status(
        app,
        if result.is_ok() {
            "確認待ち"
        } else {
            "エラー"
        },
        "Voice Edit",
    );
    result
}

async fn cancel_selected_voice_edit_recording_locked(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<bool, String> {
    if let Err(error) = hotkey::set_cancel_hotkey_enabled(false) {
        tracing::warn!("failed to disable selected voice edit Escape hotkey: {error}");
    }
    let active = state.active_selected_voice_edit.lock().await.take();
    let outcome = session_service::cancel_session_inner(state).await;
    *state.session_kind.lock().await = None;
    *state.recording_trigger.lock().await = None;
    if let Some(active) = active {
        recovery::cleanup_completed_session(app, &active.recovery_id)
            .map_err(|error| error.to_string())?;
    }
    tray::update_status(app, "待機中", "Voice Edit");
    Ok(outcome.cancelled)
}

#[tauri::command]
pub async fn toggle_selected_voice_edit(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<SelectedVoiceEditToggleResult, String> {
    let _guard = state.session_action.lock().await;
    if *state.session_kind.lock().await == Some(SessionKind::SelectedVoiceEdit) {
        return stop_selected_voice_edit_locked(&app, &state)
            .await
            .map(|preview| SelectedVoiceEditToggleResult::Preview { preview });
    }
    start_selected_voice_edit_locked(&app, &state).await
}

#[tauri::command]
pub async fn get_selected_voice_edit_status(state: State<'_, AppState>) -> Result<String, String> {
    let _guard = state.session_action.lock().await;
    if *state.session_kind.lock().await == Some(SessionKind::SelectedVoiceEdit) {
        Ok("recording".to_string())
    } else if state.pending_selected_voice_edit.lock().await.is_some() {
        Ok("preview".to_string())
    } else {
        Ok("idle".to_string())
    }
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

#[tauri::command]
pub async fn cancel_recording_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let _guard = state.session_action.lock().await;
    cancel_recording_locked(&app, &state).await
}

async fn stop_recording_preview_locked(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    let recording_state = state.recording_state.lock().await.clone();
    if !matches!(recording_state, RecordingState::Recording) {
        return Ok(String::new());
    }
    match state.commit_processing_or_observe_cancel() {
        ProcessingGateOutcome::CancelRequested => {
            cancel_recording_locked(app, state).await?;
            return Ok(String::new());
        }
        ProcessingGateOutcome::ProcessingCommitted => {}
    }
    *state.recording_state.lock().await = RecordingState::Processing;

    if let Err(error) = hotkey::set_cancel_hotkey_enabled(false) {
        tracing::warn!("failed to disable recording cancel hotkey before preview stop: {error}");
    }

    let (mode, polish_preset, _) = active_session_snapshot(state).await;
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
            polish_state: None,
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
            polish_state: result
                .as_ref()
                .ok()
                .map(|outcome| outcome.polish_state.clone()),
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

fn ensure_polish_preset_change_allowed(session_active: bool) -> Result<(), String> {
    if session_active {
        Err("録音開始時のPolishプリセットはこのセッション中は変更できません。次回の録音前に設定してください。".to_string())
    } else {
        Ok(())
    }
}

#[tauri::command]
pub async fn set_active_polish_preset(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    preset: String,
) -> Result<String, String> {
    let _guard = state.session_action.lock().await;
    let preset = normalize_polish_preset(&preset)?;
    ensure_polish_preset_change_allowed(state.session.lock().await.is_some())?;
    let mut next = state.settings.lock().await.clone();
    next.polish_preset = preset.clone();
    settings::save(&app, &next).map_err(|error| error.to_string())?;
    *state.settings.lock().await = next;
    Ok(preset)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let settings = state.settings.lock().await.clone();
    settings_for_ui(&settings)
}

#[tauri::command]
pub async fn get_data_processing_summary(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<data_flow::DataProcessingSummary, String> {
    let mut settings = state.settings.lock().await.clone();
    let focused = settings_target_context(&state).await;
    let effective = {
        let _guard = state.app_profiles_action.lock().await;
        let store = app_profiles::load(&app).map_err(|error| error.to_string())?;
        app_profiles::resolve(&store, focused.as_ref(), &settings, None)
    };
    let mode = effective.mode;
    settings.language_mode = effective.language_mode;
    settings.polish_preset = effective.polish_preset;
    let has_dictionary = !state.dictionary_words.lock().await.is_empty();
    let correction_state = if settings.correction_learning_mode == CorrectionLearningMode::Ask {
        match corrections::load(&app) {
            Ok(store) => {
                let active_items = store
                    .items
                    .iter()
                    .filter(|item| item.status == CorrectionStatus::Active)
                    .collect::<Vec<_>>();
                data_flow::CorrectionFlowState {
                    total: store.items.len(),
                    active: active_items.len(),
                    has_vocabulary: active_items.iter().any(|item| {
                        item.artifacts.iter().any(|artifact| {
                            matches!(artifact, CorrectionArtifact::Vocabulary { .. })
                        })
                    }),
                    has_replacement: active_items.iter().any(|item| {
                        item.artifacts.iter().any(|artifact| {
                            matches!(artifact, CorrectionArtifact::Replacement { .. })
                        })
                    }),
                    has_style_example: active_items.iter().any(|item| {
                        item.artifacts.iter().any(|artifact| {
                            matches!(artifact, CorrectionArtifact::StyleExample { .. })
                        })
                    }),
                    store_error: false,
                }
            }
            Err(_) => data_flow::CorrectionFlowState {
                store_error: true,
                ..Default::default()
            },
        }
    } else {
        data_flow::CorrectionFlowState::default()
    };
    Ok(data_flow::summarize_with_corrections(
        &settings,
        &mode,
        has_dictionary,
        &correction_state,
    ))
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
pub async fn inject_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<Option<String>, String> {
    if text.trim().is_empty() {
        return Err("注入するテキストがありません。".to_string());
    }
    let target = state
        .last_target_window
        .lock()
        .await
        .clone()
        .ok_or_else(|| {
            crate::inject::error_with_clipboard_backup(
                &text,
                anyhow::anyhow!("入力先アプリを一度クリックしてから再注入してください"),
            )
            .to_string()
        })?;
    dispatch_user_injection(&SystemUserInjectionBackend, &text, Some(&target))
        .map(|success| success.warning)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_history(app: tauri::AppHandle) -> Result<Vec<HistoryEntry>, String> {
    local_data::load_history(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_history_item(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<HistoryEntry>, String> {
    let _guard = state.history_action.lock().await;
    local_data::delete_history_item(&app, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn toggle_history_pin(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<HistoryEntry>, String> {
    let _guard = state.history_action.lock().await;
    local_data::toggle_history_pin(&app, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn rerun_history_polish(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<HistoryEntry>, String> {
    let history = {
        let _guard = state.history_action.lock().await;
        local_data::load_history(&app).map_err(|error| error.to_string())?
    };
    let index = history
        .iter()
        .position(|item| item.id == id)
        .ok_or_else(|| "履歴が見つかりません。".to_string())?;
    if history[index].operation_kind == OperationKind::SelectedVoiceEdit {
        return Err("選択音声編集の履歴はPolish再実行できません。".to_string());
    }
    let source_text = if !history[index].raw_text.trim().is_empty() {
        history[index].raw_text.clone()
    } else {
        history[index].final_text.clone()
    };
    if source_text.trim().is_empty() {
        return Err("Polish再実行に使えるテキストがありません。".to_string());
    }

    let mut current_settings = state.settings.lock().await.clone();
    if current_settings.api_key.is_empty() {
        return Err("APIキーが設定されていません。設定画面から入力してください。".to_string());
    }
    if !history[index].polish_preset.is_empty() {
        current_settings.polish_preset = history[index].polish_preset.clone();
    }
    let manual_dictionary = state.dictionary_words.lock().await.clone();
    let correction_store =
        if current_settings.correction_learning_mode == CorrectionLearningMode::Ask {
            let _guard = state.corrections_action.lock().await;
            corrections::load(&app).map_err(|error| error.to_string())?
        } else {
            corrections::CorrectionStore::default()
        };
    let dictionary_words =
        if current_settings.correction_learning_mode == CorrectionLearningMode::Ask {
            corrections::effective_vocabulary(
                &manual_dictionary,
                &correction_store,
                &history[index].app_process,
            )
        } else {
            manual_dictionary
        };
    let focused_context =
        if current_settings.deep_context_enabled && !history[index].app_process.is_empty() {
            Some(FocusedAppContext {
                process_name: history[index].app_process.clone(),
                window_title: String::new(),
            })
        } else {
            None
        };
    let post_asr_text = if current_settings.correction_learning_mode == CorrectionLearningMode::Ask
    {
        corrections::apply_replacements(
            &source_text,
            &correction_store,
            &history[index].app_process,
        )
    } else {
        source_text.clone()
    };
    let style_examples = corrections::select_style_examples(
        &correction_store,
        &current_settings.polish_preset,
        &history[index].app_process,
    );
    let routed = mode::route_with_style_examples(
        &Mode::Polish,
        &current_settings,
        &dictionary_words,
        focused_context.as_ref(),
        &style_examples,
        &post_asr_text,
    )
    .await;
    if let (Some(usage), Some(model)) = (routed.polish_usage, routed.polish_model_used.as_deref()) {
        if let Err(error) = local_data::record_polish_usage_only(&app, model, usage) {
            tracing::warn!("failed to record history Polish usage: {error}");
        }
    }
    let snippets = local_data::load_snippets(&app).map_err(|error| error.to_string())?;
    let final_text = local_data::expand_snippets(&routed.text, &snippets);
    let _guard = state.history_action.lock().await;
    let mut latest = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let latest_item = latest
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| "履歴が処理中に削除されました。".to_string())?;
    latest_item.raw_text = source_text;
    latest_item.final_text = final_text;
    latest_item.mode = Mode::Polish;
    latest_item.polish_state = routed.polish_state;
    latest_item.status = local_data::HistoryStatus::Success;
    latest_item.error = None;
    local_data::save_history(&app, &latest).map_err(|error| error.to_string())?;
    Ok(latest)
}

#[tauri::command]
pub async fn clear_history(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.history_action.lock().await;
    local_data::clear_history(&app).map_err(|error| error.to_string())
}

fn correction_history_entry<'a>(
    history: &'a [HistoryEntry],
    id: &str,
) -> Result<&'a HistoryEntry, String> {
    history
        .iter()
        .find(|item| {
            item.id == id
                && item.status == local_data::HistoryStatus::Success
                && item.operation_kind == OperationKind::Dictation
        })
        .ok_or_else(|| {
            "修正元の履歴が見つかりません。履歴が削除済みか、処理に失敗しています。".to_string()
        })
}

fn selected_history_entry<'a>(
    history: &'a [HistoryEntry],
    resolved: &ResolvedPendingSelection,
) -> Result<&'a HistoryEntry, String> {
    let entry = correction_history_entry(history, &resolved.candidate.id)?;
    if !selected_learning::candidate_matches_history(&resolved.candidate, entry) {
        return Err(
            "選択した履歴候補は準備後に変更されました。もう一度選択からやり直してください。"
                .to_string(),
        );
    }
    Ok(entry)
}

fn show_and_focus_main(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "KoeTypeの確認画面が見つかりません。".to_string())?;
    let _ = window.unminimize();
    window
        .show()
        .map_err(|_| "KoeTypeの確認画面を表示できませんでした。".to_string())?;
    window
        .set_focus()
        .map_err(|_| "KoeTypeの確認画面へフォーカスできませんでした。".to_string())
}

#[tauri::command]
pub fn show_main_for_selected_correction_error(app: tauri::AppHandle) -> Result<(), String> {
    show_and_focus_main(&app)
}

#[tauri::command]
pub async fn prepare_selected_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<PrepareSelectedCorrectionResult, String> {
    let _selected_guard = state.selected_learning_action.lock().await;
    *state.pending_selected_learning.lock().await = None;
    let _session_guard = state.session_action.lock().await;
    if state.active_selected_voice_edit.lock().await.is_some()
        || state.pending_selected_voice_edit.lock().await.is_some()
    {
        return Err("選択音声編集が進行中です。先に完了またはキャンセルしてください。".to_string());
    }
    let correction_settings = {
        let settings = state.settings.lock().await;
        (
            settings.correction_learning_mode,
            settings.correction_learning_multi_diff_enabled,
        )
    };
    selected_learning::ensure_learning_enabled(correction_settings.0)?;
    let recording_state = state.recording_state.lock().await.clone();
    let has_session = state.session.lock().await.is_some();
    selected_learning::ensure_session_idle(&recording_state, has_session)?;

    // KoeTypeへfocusを移す前に、外部アプリの選択範囲を取得する。
    let capture = crate::selection::capture_selected_text()
        .await
        .map_err(|error| error.to_string())?;
    let history = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let store = {
        let _guard = state.corrections_action.lock().await;
        corrections::load(&app).map_err(|error| error.to_string())?
    };
    let now = selected_learning::now_secs();
    let candidates = selected_learning::select_candidates(
        &history,
        &store,
        &capture.target.process_name,
        now,
        correction_settings.1,
    );
    if candidates.is_empty() {
        return Err("直近30分に学習元として使える音声入力履歴がありません。".to_string());
    }
    if selected_learning::all_candidates_match_selected(&candidates, &capture.text) {
        return Err(
            "選択テキストは利用可能な履歴の出力と同じため、学習する変更がありません。".to_string(),
        );
    }
    selected_learning::ensure_learning_enabled(
        state.settings.lock().await.correction_learning_mode,
    )?;
    let result = {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::replace_pending(
            &mut pending,
            capture.text,
            candidates,
            now,
            capture.target,
            capture.warning,
            correction_settings.1,
        )
    };
    if let Err(error) = show_and_focus_main(&app) {
        let mut pending = state.pending_selected_learning.lock().await;
        let _ = selected_learning::consume_pending(&mut pending, &result.token);
        return Err(error);
    }
    Ok(result)
}

#[tauri::command]
pub async fn preview_selected_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
    history_id: String,
    comparison_mode: ComparisonMode,
    source_start_utf16: Option<usize>,
    source_end_utf16: Option<usize>,
    source_display_fingerprint: String,
    record_id: Option<String>,
) -> Result<CorrectionPreview, String> {
    let _selected_guard = state.selected_learning_action.lock().await;
    selected_learning::ensure_learning_enabled(
        state.settings.lock().await.correction_learning_mode,
    )?;
    let resolved = {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::resolve_pending(
            &mut pending,
            &token,
            &history_id,
            selected_learning::now_secs(),
        )
        .and_then(|resolved| {
            selected_learning::begin_preview(&mut pending, &token, &history_id)?;
            Ok(resolved)
        })?
    };
    let history = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let source = selected_history_entry(&history, &resolved)?;
    if !resolved.multi_diff_enabled
        && (comparison_mode != ComparisonMode::Full || record_id.is_some())
    {
        return Err("複数差分機能が無効なため、従来の全文比較で新規保存してください。".to_string());
    }
    if corrections::source_display_fingerprint(&source.final_text) != source_display_fingerprint {
        return Err("元の出力表示が変更されました。履歴を選び直してください。".to_string());
    }
    let mut preview = match comparison_mode {
        ComparisonMode::Full => {
            if source_start_utf16.is_some() || source_end_utf16.is_some() {
                return Err("全文比較では元範囲を指定できません。".to_string());
            }
            if resolved.multi_diff_enabled {
                corrections::preview(
                    source.id.clone(),
                    source.final_text.clone(),
                    resolved.selected_text.clone(),
                    &source.mode,
                    source.app_process.clone(),
                )
            } else {
                corrections::preview_legacy(
                    source.id.clone(),
                    source.final_text.clone(),
                    resolved.selected_text.clone(),
                    &source.mode,
                    source.app_process.clone(),
                )
            }
        }
        ComparisonMode::ExplicitRange => {
            let (Some(start), Some(end)) = (source_start_utf16, source_end_utf16) else {
                return Err("範囲指定では確認済みの元範囲が必要です。".to_string());
            };
            corrections::preview_selected_excerpt(
                source.id.clone(),
                source.final_text.clone(),
                resolved.selected_text.clone(),
                start,
                end,
                &source.mode,
                source.app_process.clone(),
            )
        }
    }
    .map_err(|error| error.to_string())?;
    let store = {
        let _guard = state.corrections_action.lock().await;
        corrections::load(&app).map_err(|error| error.to_string())?
    };
    let records = store
        .items
        .iter()
        .filter(|record| record.source_history_id == source.id)
        .collect::<Vec<_>>();
    let target = match (record_id.as_deref(), records.as_slice()) {
        (Some(id), _) => Some(
            records
                .iter()
                .copied()
                .find(|record| record.id == id)
                .ok_or_else(|| "選択した既存修正レコードが見つかりません。".to_string())?,
        ),
        (None, []) => None,
        (None, [record]) => Some(*record),
        (None, _) => {
            return Err(
                "この履歴には複数の既存修正があります。編集対象を選択してください。".to_string(),
            )
        }
    };
    let source_records_fingerprint = corrections::source_records_fingerprint(&store, &source.id);
    let target_record_id = target.map(|record| record.id.clone());
    let record_edit_fingerprint = target.map(|record| corrections::record_edit_fingerprint(record));
    if let Some(record) = target {
        preview.persisted_artifacts = record.artifacts.clone();
        preview.target_record_corrected_text = Some(record.corrected_text.clone());
        for artifact in &record.artifacts {
            if let Some(candidate) = preview.candidates.iter_mut().find(|candidate| {
                candidate.artifact == *artifact
                    && candidate.status == corrections::CorrectionCandidateStatus::Eligible
                    && candidate.persistence_state == CorrectionCandidatePersistenceState::New
            }) {
                candidate.persistence_state =
                    CorrectionCandidatePersistenceState::PersistedVerified;
            } else {
                preview
                    .persisted_unverified_artifacts
                    .push(artifact.clone());
            }
        }
    }
    corrections::bind_selected_preview_identity(
        &mut preview,
        target_record_id.as_deref(),
        record_edit_fingerprint.as_deref(),
        &source_records_fingerprint,
    );
    preview.idempotency_key = selected_learning::new_idempotency_key();
    {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::confirm_preview(
            &mut pending,
            &token,
            &history_id,
            ConfirmedSelectedPreview {
                history_id: history_id.clone(),
                comparison_mode,
                source_start_utf16,
                source_end_utf16,
                source_display_fingerprint,
                preview_fingerprint: preview.preview_fingerprint.clone(),
                idempotency_key: preview.idempotency_key.clone(),
                target_record_id: preview.target_record_id.clone(),
                record_edit_fingerprint: preview.record_edit_fingerprint.clone(),
                source_records_fingerprint: preview.source_records_fingerprint.clone(),
                persisted_unverified_artifacts: preview.persisted_unverified_artifacts.clone(),
                persisted_artifacts: preview.persisted_artifacts.clone(),
            },
        )?;
    }
    Ok(preview)
}

#[tauri::command]
pub async fn create_selected_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
    history_id: String,
    comparison_mode: ComparisonMode,
    source_start_utf16: Option<usize>,
    source_end_utf16: Option<usize>,
    source_display_fingerprint: String,
    preview_fingerprint: String,
    idempotency_key: String,
    record_id: Option<String>,
    operation: SelectedCorrectionOperation,
    artifacts: Vec<CorrectionArtifact>,
    vocabulary_associations: Vec<VocabularyCandidateAssociation>,
) -> Result<CreateSelectedCorrectionResult, String> {
    let _selected_guard = state.selected_learning_action.lock().await;
    let request_digest = corrections::create_request_digest(
        &preview_fingerprint,
        record_id.as_deref(),
        operation.as_str(),
        &artifacts,
        &vocabulary_associations,
    );
    {
        let mut replay_cache = state.selected_learning_replays.lock().await;
        if let Some((record_id, focus_warning)) = selected_learning::check_replay(
            &mut replay_cache,
            &token,
            &idempotency_key,
            &preview_fingerprint,
            &request_digest,
            selected_learning::now_secs(),
        )? {
            return Ok(CreateSelectedCorrectionResult {
                record_id,
                replayed: true,
                focus_warning,
            });
        }
    }
    selected_learning::ensure_learning_enabled(
        state.settings.lock().await.correction_learning_mode,
    )?;
    let resolved = {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::resolve_pending(
            &mut pending,
            &token,
            &history_id,
            selected_learning::now_secs(),
        )?
    };
    let confirmed = {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::require_confirmed_preview(
            &mut pending,
            &token,
            &history_id,
            comparison_mode,
            source_start_utf16,
            source_end_utf16,
            &source_display_fingerprint,
            &preview_fingerprint,
            &idempotency_key,
            record_id.as_deref(),
        )?
    };
    let history = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let source = selected_history_entry(&history, &resolved)?;
    if !resolved.multi_diff_enabled
        && (comparison_mode != ComparisonMode::Full
            || record_id.is_some()
            || operation != SelectedCorrectionOperation::Create)
    {
        return Err("複数差分機能が無効なため、従来の全文比較で新規保存してください。".to_string());
    }
    if corrections::source_display_fingerprint(&source.final_text) != source_display_fingerprint {
        return Err("元の出力表示が変更されました。再プレビューしてください。".to_string());
    }
    let mut preview = match comparison_mode {
        ComparisonMode::Full => {
            if resolved.multi_diff_enabled {
                corrections::preview(
                    source.id.clone(),
                    source.final_text.clone(),
                    resolved.selected_text.clone(),
                    &source.mode,
                    source.app_process.clone(),
                )
            } else {
                corrections::preview_legacy(
                    source.id.clone(),
                    source.final_text.clone(),
                    resolved.selected_text.clone(),
                    &source.mode,
                    source.app_process.clone(),
                )
            }
        }
        ComparisonMode::ExplicitRange => {
            let (Some(start), Some(end)) = (source_start_utf16, source_end_utf16) else {
                return Err("範囲指定では確認済みの元範囲が必要です。".to_string());
            };
            corrections::preview_selected_excerpt(
                source.id.clone(),
                source.final_text.clone(),
                resolved.selected_text.clone(),
                start,
                end,
                &source.mode,
                source.app_process.clone(),
            )
        }
    }
    .map_err(|error| error.to_string())?;
    corrections::bind_selected_preview_identity(
        &mut preview,
        confirmed.target_record_id.as_deref(),
        confirmed.record_edit_fingerprint.as_deref(),
        &confirmed.source_records_fingerprint,
    );
    if preview.preview_fingerprint != preview_fingerprint {
        return Err("プレビュー内容が古くなりました。もう一度プレビューしてください。".to_string());
    }
    if resolved.multi_diff_enabled
        && !matches!(
            operation,
            SelectedCorrectionOperation::Delete | SelectedCorrectionOperation::None
        )
    {
        let artifacts_to_validate_with_indexes =
            corrections::changed_artifacts_with_indexes(&confirmed.persisted_artifacts, &artifacts);
        let artifacts_to_validate = artifacts_to_validate_with_indexes
            .iter()
            .map(|(_, artifact)| artifact.clone())
            .collect::<Vec<_>>();
        if !artifacts_to_validate.is_empty() {
            if vocabulary_associations.is_empty() {
                corrections::validate_selected_preview_artifacts(&preview, &artifacts_to_validate)
            } else {
                let mapped_associations = vocabulary_associations
                    .iter()
                    .map(|association| {
                        let artifact_index = artifacts_to_validate_with_indexes
                            .iter()
                            .position(|(original_index, _)| {
                                *original_index == association.artifact_index
                            })
                            .ok_or_else(|| {
                                "保持中の旧artifactへ語彙対応を付け直すことはできません。"
                                    .to_string()
                            })?;
                        Ok(VocabularyCandidateAssociation {
                            artifact_index,
                            candidate_id: association.candidate_id.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                corrections::validate_selected_preview_artifacts_with_associations(
                    &preview,
                    &artifacts_to_validate,
                    &mapped_associations,
                )
            }
            .map_err(|error| error.to_string())?;
        } else if artifacts.is_empty() {
            return Err("保存する候補を1件以上選択してください。".to_string());
        }
    }
    let configured_api_key = state.settings.lock().await.api_key.clone();
    corrections::reject_configured_api_key(
        &configured_api_key,
        &source.raw_text,
        &source.final_text,
        &preview.corrected_text,
        &source.polish_preset,
        &source.app_process,
        &artifacts,
    )
    .map_err(|error| error.to_string())?;

    let record_id = {
        let _guard = state.corrections_action.lock().await;
        let mut store = corrections::load(&app).map_err(|error| error.to_string())?;
        if corrections::source_records_fingerprint(&store, &source.id)
            != confirmed.source_records_fingerprint
        {
            return Err(
                "修正学習データが別画面で変更されました。再読み込みしてください。".to_string(),
            );
        }
        let result_id = match operation {
            SelectedCorrectionOperation::Create => {
                if confirmed.target_record_id.is_some()
                    || store
                        .items
                        .iter()
                        .any(|record| record.source_history_id == source.id)
                {
                    return Err(
                        "この履歴には既存の修正があります。再編集としてプレビューしてください。"
                            .to_string(),
                    );
                }
                corrections::insert(
                    &mut store,
                    NewCorrection {
                        source_history_id: source.id.clone(),
                        raw_text: source.raw_text.clone(),
                        original_text: source.final_text.clone(),
                        corrected_text: preview.corrected_text.clone(),
                        mode: source.mode.clone(),
                        polish_preset: source.polish_preset.clone(),
                        app_process: source.app_process.clone(),
                        classification: preview.classification,
                        artifacts,
                    },
                )
                .map_err(|error| error.to_string())?
                .id
            }
            SelectedCorrectionOperation::Update | SelectedCorrectionOperation::None => {
                let target_id = confirmed
                    .target_record_id
                    .as_deref()
                    .ok_or_else(|| "更新対象の修正レコードがありません。".to_string())?;
                let current = store
                    .items
                    .iter()
                    .find(|record| record.id == target_id)
                    .ok_or_else(|| "更新対象の修正レコードが見つかりません。".to_string())?;
                if Some(corrections::record_edit_fingerprint(current))
                    != confirmed.record_edit_fingerprint
                {
                    return Err(
                        "修正レコードが別画面で更新されました。再読み込みしてください。"
                            .to_string(),
                    );
                }
                let next_artifacts = if operation == SelectedCorrectionOperation::None {
                    vec![CorrectionArtifact::None]
                } else {
                    artifacts
                };
                corrections::update(
                    &mut store,
                    target_id,
                    UpdateCorrection {
                        corrected_text: preview.corrected_text.clone(),
                        classification: preview.classification,
                        artifacts: next_artifacts,
                    },
                )
                .map_err(|error| error.to_string())?
                .id
            }
            SelectedCorrectionOperation::Delete => {
                let target_id = confirmed
                    .target_record_id
                    .as_deref()
                    .ok_or_else(|| "削除対象の修正レコードがありません。".to_string())?;
                let current = store
                    .items
                    .iter()
                    .find(|record| record.id == target_id)
                    .ok_or_else(|| "削除対象の修正レコードが見つかりません。".to_string())?;
                if Some(corrections::record_edit_fingerprint(current))
                    != confirmed.record_edit_fingerprint
                {
                    return Err(
                        "修正レコードが別画面で更新されました。再読み込みしてください。"
                            .to_string(),
                    );
                }
                corrections::delete(&mut store, target_id).map_err(|error| error.to_string())?;
                target_id.to_string()
            }
        };
        corrections::save(&app, &store).map_err(|error| error.to_string())?;
        result_id
    };
    let target = {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::consume_pending(&mut pending, &token)
    };
    let focus_warning = target
        .map(|target| selected_focus_warning(context::focus_window(&target)))
        .unwrap_or_else(|| {
            Some("修正内容は保存しましたが、元の入力先情報が失われました。".to_string())
        });
    state
        .selected_learning_replays
        .lock()
        .await
        .push(SelectedLearningReplay {
            token,
            idempotency_key,
            preview_fingerprint,
            create_request_digest: request_digest,
            record_id: record_id.clone(),
            focus_warning: focus_warning.clone(),
            created_at: selected_learning::now_secs(),
        });
    Ok(CreateSelectedCorrectionResult {
        record_id,
        replayed: false,
        focus_warning,
    })
}

#[tauri::command]
pub async fn revalidate_selected_correction_artifacts(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
    history_id: String,
    comparison_mode: ComparisonMode,
    source_start_utf16: Option<usize>,
    source_end_utf16: Option<usize>,
    source_display_fingerprint: String,
    preview_fingerprint: String,
    idempotency_key: String,
    record_id: Option<String>,
    artifacts: Vec<CorrectionArtifact>,
    vocabulary_associations: Vec<VocabularyCandidateAssociation>,
) -> Result<(), String> {
    let _selected_guard = state.selected_learning_action.lock().await;
    let resolved = {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::resolve_pending(
            &mut pending,
            &token,
            &history_id,
            selected_learning::now_secs(),
        )?
    };
    let confirmed = {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::require_confirmed_preview(
            &mut pending,
            &token,
            &history_id,
            comparison_mode,
            source_start_utf16,
            source_end_utf16,
            &source_display_fingerprint,
            &preview_fingerprint,
            &idempotency_key,
            record_id.as_deref(),
        )?
    };
    let history = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let source = selected_history_entry(&history, &resolved)?;
    let mut preview = match comparison_mode {
        ComparisonMode::Full => {
            if resolved.multi_diff_enabled {
                corrections::preview(
                    source.id.clone(),
                    source.final_text.clone(),
                    resolved.selected_text.clone(),
                    &source.mode,
                    source.app_process.clone(),
                )
            } else {
                corrections::preview_legacy(
                    source.id.clone(),
                    source.final_text.clone(),
                    resolved.selected_text.clone(),
                    &source.mode,
                    source.app_process.clone(),
                )
            }
        }
        ComparisonMode::ExplicitRange => {
            let (Some(start), Some(end)) = (source_start_utf16, source_end_utf16) else {
                return Err("範囲指定では確認済みの元範囲が必要です。".to_string());
            };
            corrections::preview_selected_excerpt(
                source.id.clone(),
                source.final_text.clone(),
                resolved.selected_text.clone(),
                start,
                end,
                &source.mode,
                source.app_process.clone(),
            )
        }
    }
    .map_err(|error| error.to_string())?;
    corrections::bind_selected_preview_identity(
        &mut preview,
        confirmed.target_record_id.as_deref(),
        confirmed.record_edit_fingerprint.as_deref(),
        &confirmed.source_records_fingerprint,
    );
    if preview.preview_fingerprint != preview_fingerprint {
        return Err("プレビュー内容が古くなりました。もう一度プレビューしてください。".to_string());
    }
    let store = {
        let _guard = state.corrections_action.lock().await;
        let store = corrections::load(&app).map_err(|error| error.to_string())?;
        corrections::validate_confirmed_store_state(
            &store,
            &source.id,
            &confirmed.source_records_fingerprint,
            confirmed.target_record_id.as_deref(),
            confirmed.record_edit_fingerprint.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        store
    };
    let artifacts_to_validate_with_indexes =
        corrections::changed_artifacts_with_indexes(&confirmed.persisted_artifacts, &artifacts);
    let artifacts_to_validate = artifacts_to_validate_with_indexes
        .iter()
        .map(|(_, artifact)| artifact.clone())
        .collect::<Vec<_>>();
    let changed_artifact_indexes = artifacts_to_validate_with_indexes
        .iter()
        .map(|(index, _)| *index)
        .collect::<std::collections::HashSet<_>>();
    let retained_artifacts = artifacts
        .iter()
        .enumerate()
        .filter_map(|(index, artifact)| {
            (!changed_artifact_indexes.contains(&index)).then_some(artifact.clone())
        })
        .collect::<Vec<_>>();
    let mapped_associations = vocabulary_associations
        .iter()
        .map(|association| {
            let artifact_index = artifacts_to_validate_with_indexes
                .iter()
                .position(|(original_index, _)| *original_index == association.artifact_index)
                .ok_or_else(|| {
                    "保持中の旧artifactへ語彙対応を付け直すことはできません。".to_string()
                })?;
            Ok(VocabularyCandidateAssociation {
                artifact_index,
                candidate_id: association.candidate_id.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if resolved.multi_diff_enabled {
        corrections::validate_selected_preview_artifacts_with_associations(
            &preview,
            &artifacts_to_validate,
            &mapped_associations,
        )
    } else {
        corrections::validate_preview_artifacts(&preview, &artifacts_to_validate)
    }
    .map_err(|error| error.to_string())?;
    corrections::validate_changed_artifact_replacement_conflicts(
        &store,
        &artifacts_to_validate,
        &retained_artifacts,
        &source.app_process,
        confirmed.target_record_id.as_deref(),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_selected_correction(
    state: State<'_, AppState>,
    token: String,
) -> Result<(), String> {
    let _selected_guard = state.selected_learning_action.lock().await;
    let target = {
        let mut pending = state.pending_selected_learning.lock().await;
        selected_learning::cancel_pending(&mut pending, &token)?
    };
    context::focus_window(&target).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn replace_selected_voice_edit(
    _app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> Result<SelectedVoiceEditReplaceResult, String> {
    let _guard = state.session_action.lock().await;
    let resolved = {
        let mut pending = state.pending_selected_voice_edit.lock().await;
        selected_voice_edit::resolve_pending(&mut pending, &token, selected_voice_edit::now_secs())?
    };
    if let Err(error) = context::focus_window(&resolved.target) {
        return Ok(SelectedVoiceEditReplaceResult {
            replaced: false,
            code: "focus_mismatch".to_string(),
            message: format!("元の入力先へフォーカスを戻せないため置換していません: {error}"),
            partial: false,
        });
    }
    let current = match crate::selection::capture_selected_text().await {
        Ok(capture) => capture,
        Err(error) => {
            return Ok(SelectedVoiceEditReplaceResult {
                replaced: false,
                code: "selection_unavailable".to_string(),
                message: format!("選択範囲を再確認できないため置換していません: {error}"),
                partial: false,
            });
        }
    };
    let decision = selected_voice_edit::replacement_decision(&resolved, &current);
    if decision != ReplaceDecision::Replace {
        let (code, message) = match decision {
            ReplaceDecision::ClipboardCaptureUnsupported => (
                "clipboard_capture_unsupported",
                "安全な直接置換に必要なUI Automation選択を確認できません。Copyで提案を回収してください。",
            ),
            ReplaceDecision::TargetMismatch => (
                "target_mismatch",
                "入力先ウィンドウが開始時と異なるため置換していません。",
            ),
            ReplaceDecision::SelectionMismatch => (
                "selection_mismatch",
                "選択本文が開始時から変わったため置換していません。",
            ),
            ReplaceDecision::Replace => unreachable!(),
        };
        return Ok(SelectedVoiceEditReplaceResult {
            replaced: false,
            code: code.to_string(),
            message: message.to_string(),
            partial: false,
        });
    }
    match crate::inject::inject_clipboard_replacement(&resolved.proposal, &resolved.target) {
        Ok(success) => {
            let mut pending = state.pending_selected_voice_edit.lock().await;
            selected_voice_edit::finish_pending(&mut pending, &token)?;
            Ok(SelectedVoiceEditReplaceResult {
                replaced: true,
                code: "replaced".to_string(),
                message: success
                    .warning
                    .unwrap_or_else(|| "選択範囲を提案文へ置換しました。".to_string()),
                partial: false,
            })
        }
        Err(failure) => Ok(SelectedVoiceEditReplaceResult {
            replaced: false,
            code: if failure.partial {
                "partial_injection"
            } else {
                "injection_failed"
            }
            .to_string(),
            message: failure.message,
            partial: failure.partial,
        }),
    }
}

#[tauri::command]
pub async fn cancel_selected_voice_edit_preview(
    state: State<'_, AppState>,
    token: String,
) -> Result<(), String> {
    let _guard = state.session_action.lock().await;
    let target = {
        let mut pending = state.pending_selected_voice_edit.lock().await;
        selected_voice_edit::cancel_pending(&mut pending, &token)?
    };
    context::focus_window(&target).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn preview_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    history_id: String,
    corrected_text: String,
) -> Result<CorrectionPreview, String> {
    let settings = state.settings.lock().await.clone();
    if settings.correction_learning_mode == CorrectionLearningMode::Off {
        return Err("修正学習はオフです。設定で「保存前に確認」を選択してください。".to_string());
    }
    let history = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let source = correction_history_entry(&history, &history_id)?;
    let preview = if settings.correction_learning_multi_diff_enabled {
        corrections::preview(
            history_id,
            source.final_text.clone(),
            corrected_text,
            &source.mode,
            source.app_process.clone(),
        )
    } else {
        corrections::preview_legacy(
            history_id,
            source.final_text.clone(),
            corrected_text,
            &source.mode,
            source.app_process.clone(),
        )
    };
    preview.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn revalidate_correction_artifacts(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    history_id: String,
    corrected_text: String,
    artifacts: Vec<CorrectionArtifact>,
) -> Result<(), String> {
    let settings = state.settings.lock().await.clone();
    if settings.correction_learning_mode == CorrectionLearningMode::Off {
        return Err("修正学習はオフです。".to_string());
    }
    let history = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let source = correction_history_entry(&history, &history_id)?;
    let preview = if settings.correction_learning_multi_diff_enabled {
        corrections::preview(
            history_id,
            source.final_text.clone(),
            corrected_text,
            &source.mode,
            source.app_process.clone(),
        )
    } else {
        corrections::preview_legacy(
            history_id,
            source.final_text.clone(),
            corrected_text,
            &source.mode,
            source.app_process.clone(),
        )
    }
    .map_err(|error| error.to_string())?;
    if settings.correction_learning_multi_diff_enabled {
        corrections::validate_multi_diff_preview_artifacts(&preview, &artifacts)
            .map_err(|error| error.to_string())?;
    } else {
        corrections::validate_preview_artifacts(&preview, &artifacts)
            .map_err(|error| error.to_string())?;
    }
    let _guard = state.corrections_action.lock().await;
    let store = corrections::load(&app).map_err(|error| error.to_string())?;
    corrections::validate_changed_artifact_replacement_conflicts(
        &store,
        &artifacts,
        &[],
        &source.app_process,
        None,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_corrections(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<CorrectionRecord>, String> {
    let _guard = state.corrections_action.lock().await;
    corrections::load(&app)
        .map(|store| store.items)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    history_id: String,
    corrected_text: String,
    artifacts: Vec<CorrectionArtifact>,
) -> Result<CorrectionRecord, String> {
    let settings = state.settings.lock().await.clone();
    if settings.correction_learning_mode == CorrectionLearningMode::Off {
        return Err("修正学習はオフです。設定で「保存前に確認」を選択してください。".to_string());
    }
    let history = local_data::load_history(&app).map_err(|error| error.to_string())?;
    let source = correction_history_entry(&history, &history_id)?;
    let preview = if settings.correction_learning_multi_diff_enabled {
        corrections::preview(
            history_id.clone(),
            source.final_text.clone(),
            corrected_text.clone(),
            &source.mode,
            source.app_process.clone(),
        )
    } else {
        corrections::preview_legacy(
            history_id.clone(),
            source.final_text.clone(),
            corrected_text.clone(),
            &source.mode,
            source.app_process.clone(),
        )
    }
    .map_err(|error| error.to_string())?;
    if settings.correction_learning_multi_diff_enabled {
        corrections::validate_multi_diff_preview_artifacts(&preview, &artifacts)
            .map_err(|error| error.to_string())?;
    }
    let configured_api_key = settings.api_key;
    corrections::reject_configured_api_key(
        &configured_api_key,
        &source.raw_text,
        &source.final_text,
        &corrected_text,
        &source.polish_preset,
        &source.app_process,
        &artifacts,
    )
    .map_err(|error| error.to_string())?;
    let _guard = state.corrections_action.lock().await;
    let mut store = corrections::load(&app).map_err(|error| error.to_string())?;
    let record = corrections::insert(
        &mut store,
        NewCorrection {
            source_history_id: history_id,
            raw_text: source.raw_text.clone(),
            original_text: source.final_text.clone(),
            corrected_text,
            mode: source.mode.clone(),
            polish_preset: source.polish_preset.clone(),
            app_process: source.app_process.clone(),
            classification: preview.classification,
            artifacts,
        },
    )
    .map_err(|error| error.to_string())?;
    corrections::save(&app, &store).map_err(|error| error.to_string())?;
    Ok(record)
}

#[tauri::command]
pub async fn update_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    corrected_text: String,
    artifacts: Vec<CorrectionArtifact>,
) -> Result<CorrectionRecord, String> {
    let _guard = state.corrections_action.lock().await;
    let mut store = corrections::load(&app).map_err(|error| error.to_string())?;
    let existing = store
        .items
        .iter()
        .find(|item| item.id == id)
        .cloned()
        .ok_or_else(|| "修正学習データが見つかりません。".to_string())?;
    let classification = corrections::classify(&existing.original_text, &corrected_text);
    let configured_api_key = state.settings.lock().await.api_key.clone();
    corrections::reject_configured_api_key(
        &configured_api_key,
        &existing.raw_text,
        &existing.original_text,
        &corrected_text,
        &existing.polish_preset,
        &existing.app_process,
        &artifacts,
    )
    .map_err(|error| error.to_string())?;
    let record = corrections::update(
        &mut store,
        &id,
        UpdateCorrection {
            corrected_text,
            classification,
            artifacts,
        },
    )
    .map_err(|error| error.to_string())?;
    corrections::save(&app, &store).map_err(|error| error.to_string())?;
    Ok(record)
}

async fn set_correction_status_command(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    status: CorrectionStatus,
) -> Result<Vec<CorrectionRecord>, String> {
    let _guard = state.corrections_action.lock().await;
    let mut store = corrections::load(app).map_err(|error| error.to_string())?;
    corrections::set_status(&mut store, id, status).map_err(|error| error.to_string())?;
    corrections::save(app, &store).map_err(|error| error.to_string())?;
    Ok(store.items)
}

#[tauri::command]
pub async fn undo_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<CorrectionRecord>, String> {
    set_correction_status_command(&app, &state, &id, CorrectionStatus::Undone).await
}

#[tauri::command]
pub async fn reactivate_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<CorrectionRecord>, String> {
    set_correction_status_command(&app, &state, &id, CorrectionStatus::Active).await
}

#[tauri::command]
pub async fn delete_correction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<CorrectionRecord>, String> {
    let _guard = state.corrections_action.lock().await;
    let mut store = corrections::load(&app).map_err(|error| error.to_string())?;
    corrections::delete(&mut store, &id).map_err(|error| error.to_string())?;
    corrections::save(&app, &store).map_err(|error| error.to_string())?;
    Ok(store.items)
}

#[tauri::command]
pub async fn clear_corrections(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.corrections_action.lock().await;
    // 唯一の破損復旧経路。通常loadを先に呼ばず、ユーザーの明示操作で空v1へ置換する。
    corrections::clear(&app).map_err(|error| error.to_string())
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
    if meta.operation_kind == OperationKind::SelectedVoiceEdit {
        return Err(
            "選択音声編集は原選択本文を保存しないため、Recoveryから再実行できません。".to_string(),
        );
    }
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

    let mut current_settings = state.settings.lock().await.clone();
    if current_settings.api_key.is_empty() {
        let error = "APIキーが設定されていません。設定画面から入力してください。".to_string();
        let _ = recovery::mark_failed(&app, &id, error.clone());
        return Err(error);
    }
    if !meta.polish_preset.is_empty() {
        current_settings.polish_preset = meta.polish_preset.clone();
    }
    let manual_dictionary = state.dictionary_words.lock().await.clone();
    // Recoveryは本文snapshotを重複保存せず、再実行開始時点のstoreを録音時app/presetへ適用する。
    let app_process = meta.app_process.clone();
    let correction_store =
        if current_settings.correction_learning_mode == CorrectionLearningMode::Ask {
            let _guard = state.corrections_action.lock().await;
            corrections::load(&app).map_err(|error| error.to_string())?
        } else {
            corrections::CorrectionStore::default()
        };
    let dictionary_words =
        if current_settings.correction_learning_mode == CorrectionLearningMode::Ask {
            corrections::effective_vocabulary(&manual_dictionary, &correction_store, &app_process)
        } else {
            manual_dictionary
        };
    let focused_context = if current_settings.deep_context_enabled && !app_process.is_empty() {
        Some(FocusedAppContext {
            process_name: app_process.clone(),
            window_title: String::new(),
        })
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
        language_mode: current_settings.language_mode,
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
    let post_asr_text = if current_settings.correction_learning_mode == CorrectionLearningMode::Ask
    {
        corrections::apply_replacements(&raw_text, &correction_store, &app_process)
    } else {
        raw_text.clone()
    };
    let style_examples = corrections::select_style_examples(
        &correction_store,
        &current_settings.polish_preset,
        &app_process,
    );
    let routed = mode::route_with_style_examples(
        &meta.mode,
        &current_settings,
        &dictionary_words,
        focused_context.as_ref(),
        &style_examples,
        &post_asr_text,
    )
    .await;
    if let (Some(usage), Some(model)) = (routed.polish_usage, routed.polish_model_used.as_deref()) {
        if let Err(error) = local_data::record_polish_usage_only(&app, model, usage) {
            tracing::warn!("failed to record recovery Polish usage: {error}");
        }
    }
    let snippets = local_data::load_snippets(&app).map_err(|error| error.to_string())?;
    let final_text = local_data::expand_snippets(&routed.text, &snippets);
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
    if meta.operation_kind == OperationKind::SelectedVoiceEdit {
        return Err(
            "選択音声編集のRecovery結果は自動再注入できません。Copyで回収してください。"
                .to_string(),
        );
    }
    if meta.final_text.trim().is_empty() {
        return Err("再注入できるテキストがありません。".to_string());
    }
    let target = state
        .last_target_window
        .lock()
        .await
        .clone()
        .ok_or_else(|| {
            crate::inject::error_with_clipboard_backup(
                &meta.final_text,
                anyhow::anyhow!("入力先アプリを一度クリックしてから再注入してください"),
            )
            .to_string()
        })?;
    let injection =
        dispatch_user_injection(&SystemUserInjectionBackend, &meta.final_text, Some(&target));
    let injection_warning = match injection {
        Ok(success) => success.warning,
        Err(error) => {
            let error = error.to_string();
            let _ = recovery::mark_failed(&app, &id, error.clone());
            return Err(error);
        }
    };
    recovery::update_meta(&app, &id, |meta| {
        meta.status = recovery::RecoveryStatus::TextReady;
        meta.error = None;
    })
    .map_err(|error| error.to_string())?;
    let mut summary = recovery::summarize(&app, &id).map_err(|error| error.to_string())?;
    summary.injection_warning = injection_warning;
    Ok(summary)
}

#[tauri::command]
pub async fn save_recovery_session_to_history(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryEntry, String> {
    let meta = recovery::load_meta(&app, &id).map_err(|error| error.to_string())?;
    if meta.final_text.trim().is_empty() {
        return Err("履歴に保存できるテキストがありません。".to_string());
    }
    let entry = if meta.operation_kind == OperationKind::SelectedVoiceEdit {
        local_data::make_selected_voice_edit_history_entry(
            meta.raw_text.clone(),
            meta.final_text.clone(),
            meta.duration_ms,
            None,
            meta.app_process.clone(),
        )
    } else {
        local_data::make_history_entry_with_context(
            meta.raw_text.clone(),
            meta.final_text.clone(),
            meta.mode.clone(),
            meta.duration_ms,
            None,
            if matches!(meta.mode, Mode::Polish) {
                PolishState::Unknown
            } else {
                PolishState::NotRequested
            },
            meta.polish_preset.clone(),
            meta.app_process.clone(),
        )
    };
    let saved = append_history_locked(&state, &app, entry)
        .await
        .map_err(|error| error.to_string())?;
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

async fn settings_target_context(state: &AppState) -> Option<FocusedAppContext> {
    let target = match context::current_external_focused_window() {
        Some(target) => Some(target),
        None => state.last_target_window.lock().await.clone(),
    };
    target.map(|target| FocusedAppContext {
        process_name: target.process_name,
        window_title: target.window_title,
    })
}

#[tauri::command]
pub async fn get_focused_app_context(
    state: State<'_, AppState>,
) -> Result<Option<FocusedAppContext>, String> {
    Ok(settings_target_context(&state).await)
}

#[tauri::command]
pub async fn get_app_profiles(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<AppProfile>, String> {
    let _guard = state.app_profiles_action.lock().await;
    app_profiles::load(&app)
        .map(|store| store.items)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_app_profile_conflict_warnings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: AppProfileInput,
    exclude_id: Option<String>,
) -> Result<Vec<String>, String> {
    let _guard = state.app_profiles_action.lock().await;
    let store = app_profiles::load(&app).map_err(|error| error.to_string())?;
    Ok(app_profiles::conflict_warnings(
        &store,
        &input,
        exclude_id.as_deref(),
    ))
}

#[tauri::command]
pub async fn create_app_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: AppProfileInput,
) -> Result<ProfileMutationResult, String> {
    let api_key = state.settings.lock().await.api_key.clone();
    app_profiles::reject_configured_api_key(&api_key, &input).map_err(|error| error.to_string())?;
    let _guard = state.app_profiles_action.lock().await;
    let mut store = app_profiles::load(&app).map_err(|error| error.to_string())?;
    let result = app_profiles::insert(&mut store, input).map_err(|error| error.to_string())?;
    app_profiles::save(&app, &store).map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub async fn update_app_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: AppProfileInput,
) -> Result<ProfileMutationResult, String> {
    let api_key = state.settings.lock().await.api_key.clone();
    app_profiles::reject_configured_api_key(&api_key, &input).map_err(|error| error.to_string())?;
    let _guard = state.app_profiles_action.lock().await;
    let mut store = app_profiles::load(&app).map_err(|error| error.to_string())?;
    let result = app_profiles::update(&mut store, &id, input).map_err(|error| error.to_string())?;
    app_profiles::save(&app, &store).map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub async fn set_app_profile_enabled(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<AppProfile, String> {
    let _guard = state.app_profiles_action.lock().await;
    let mut store = app_profiles::load(&app).map_err(|error| error.to_string())?;
    let item = store
        .items
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| "プロファイルが見つかりません。".to_string())?;
    item.enabled = enabled;
    item.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let result = item.clone();
    app_profiles::save(&app, &store).map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub async fn delete_app_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<AppProfile>, String> {
    let _guard = state.app_profiles_action.lock().await;
    let mut store = app_profiles::load(&app).map_err(|error| error.to_string())?;
    app_profiles::delete(&mut store, &id).map_err(|error| error.to_string())?;
    app_profiles::save(&app, &store).map_err(|error| error.to_string())?;
    Ok(store.items)
}

#[tauri::command]
pub async fn clear_app_profiles(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _guard = state.app_profiles_action.lock().await;
    app_profiles::clear(&app).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn preview_current_app_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<EffectiveAppProfile, String> {
    let focused = settings_target_context(&state).await;
    let settings = state.settings.lock().await.clone();
    let _guard = state.app_profiles_action.lock().await;
    let store = app_profiles::load(&app).map_err(|error| error.to_string())?;
    Ok(app_profiles::resolve(
        &store,
        focused.as_ref(),
        &settings,
        None,
    ))
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
    new_settings.learn_selected_hotkey = normalized.learn_selected;
    new_settings.voice_edit_selected_hotkey = normalized.voice_edit_selected;
    let startup_changed = previous.launch_at_login != new_settings.launch_at_login;
    if startup_changed {
        if let Err(error) = crate::startup::set_launch_at_login(new_settings.launch_at_login) {
            let hotkey_restore_error = hotkey::reconfigure_hotkeys(hotkey_set(&previous)).err();
            return Err(startup_change_error(
                &error.to_string(),
                hotkey_restore_error,
            ));
        }
    }

    if let Err(error) = settings::save(&app, &new_settings) {
        let hotkey_restore_error = hotkey::reconfigure_hotkeys(hotkey_set(&previous)).err();
        let startup_restore_error = startup_changed
            .then(|| crate::startup::set_launch_at_login(previous.launch_at_login).err())
            .flatten();
        let _ = settings::save(&app, &previous);
        return Err(settings_rollback_error(
            &error.to_string(),
            hotkey_restore_error,
            startup_restore_error.map(|error| error.to_string()),
        ));
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
        cell::Cell,
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Instant,
    };

    use super::{
        cleanup_api_key_import_file, complete_dictation_cancel, correction_history_entry,
        dispatch_user_injection, ensure_polish_preset_change_allowed, extract_openai_api_key_env,
        hands_free_action, list_models_with_key, parse_model_ids, push_to_talk_down_action,
        push_to_talk_up_action, read_api_key_from_env_file, recording_cancel_route, select_api_key,
        selected_focus_warning, settings_for_ui, settings_rollback_error,
        should_start_realtime_asr, startup_change_error, RecordingCancelRoute, ShortcutAction,
        UserInjectionBackend,
    };

    #[derive(Debug, PartialEq, Eq)]
    enum RecordedInjection {
        Current {
            text: String,
        },
        Target {
            text: String,
            target: crate::context::FocusedWindowTarget,
        },
    }

    #[derive(Default)]
    struct RecordingInjectionBackend(std::cell::RefCell<Vec<RecordedInjection>>);

    impl UserInjectionBackend for RecordingInjectionBackend {
        fn inject_current(&self, text: &str) -> anyhow::Result<crate::inject::InjectionSuccess> {
            self.0.borrow_mut().push(RecordedInjection::Current {
                text: text.to_string(),
            });
            Ok(crate::inject::InjectionSuccess { warning: None })
        }

        fn inject_target(
            &self,
            text: &str,
            target: &crate::context::FocusedWindowTarget,
        ) -> anyhow::Result<crate::inject::InjectionSuccess> {
            self.0.borrow_mut().push(RecordedInjection::Target {
                text: text.to_string(),
                target: target.clone(),
            });
            Ok(crate::inject::InjectionSuccess { warning: None })
        }
    }

    #[test]
    fn shared_user_injection_dispatch_forwards_all_call_sites() {
        let backend = RecordingInjectionBackend::default();
        let target = crate::context::FocusedWindowTarget {
            hwnd: 42,
            process_id: 7,
            process_name: "notepad.exe".to_string(),
            window_title: "note".to_string(),
        };

        dispatch_user_injection(&backend, "recording", None).unwrap();
        dispatch_user_injection(&backend, "history", Some(&target)).unwrap();
        dispatch_user_injection(&backend, "recovery", Some(&target)).unwrap();

        assert_eq!(
            *backend.0.borrow(),
            vec![
                RecordedInjection::Current {
                    text: "recording".to_string(),
                },
                RecordedInjection::Target {
                    text: "history".to_string(),
                    target: target.clone(),
                },
                RecordedInjection::Target {
                    text: "recovery".to_string(),
                    target,
                },
            ]
        );
    }

    #[tokio::test]
    async fn recovery_cleanup_failure_keeps_cancelled_session_idle() {
        use tokio::sync::watch;

        let state = crate::state::AppState::default();
        let (stop_tx, stop_rx) = watch::channel(false);
        let capture_task = tokio::task::spawn_blocking(move || {
            drop(stop_rx);
            Ok::<crate::audio::CapturedAudio, anyhow::Error>(Default::default())
        });
        *state.session.lock().await = Some(crate::state::SessionController {
            stop_tx,
            capture_task,
            realtime_task: None,
            started_at: Instant::now(),
            recovery_id: Some("rec-1-2".to_string()),
            mode: crate::state::Mode::Raw,
            polish_preset: "memo".to_string(),
            app_process: "notepad.exe".to_string(),
            correction_snapshot: crate::corrections::CorrectionSessionSnapshot::default(),
            language_mode: crate::settings::LanguageMode::Auto,
            target_window: None,
        });
        *state.recording_state.lock().await = crate::state::RecordingState::Recording;

        let outcome = crate::session_service::cancel_session_inner(&state).await;
        let cleanup_calls = Cell::new(0);
        let result = complete_dictation_cancel(&outcome, |_| {
            cleanup_calls.set(cleanup_calls.get() + 1);
            anyhow::bail!("locked recovery directory")
        });

        assert!(result.unwrap_err().contains("一時データを削除できません"));
        assert_eq!(cleanup_calls.get(), 1);
        assert!(matches!(
            *state.recording_state.lock().await,
            crate::state::RecordingState::Idle
        ));
        assert!(state.session.lock().await.is_none());
    }

    #[test]
    fn onboarding_dictation_session_uses_the_regular_cancel_route() {
        assert_eq!(
            recording_cancel_route(Some(crate::state::SessionKind::Dictation)),
            RecordingCancelRoute::Dictation
        );
        assert_eq!(
            recording_cancel_route(Some(crate::state::SessionKind::SelectedVoiceEdit)),
            RecordingCancelRoute::SelectedVoiceEdit
        );
    }

    #[test]
    fn correction_learning_rejects_a_missing_source_history() {
        let error = correction_history_entry(&[], "deleted-history").unwrap_err();
        assert!(error.contains("履歴が見つかりません"));
    }

    #[test]
    fn settings_rollback_error_combines_hotkey_and_startup_failures() {
        let message = settings_rollback_error(
            "store failed",
            Some("hotkey failed".to_string()),
            Some("startup failed".to_string()),
        );
        assert!(message.contains("store failed"));
        assert!(message.contains("hotkey failed"));
        assert!(message.contains("startup failed"));
    }

    #[test]
    fn startup_change_error_preserves_hotkey_restore_failure() {
        let combined =
            startup_change_error("registry failed", Some("hotkey restore failed".into()));
        assert!(combined.contains("registry failed"));
        assert!(combined.contains("hotkey restore failed"));
        let startup_only = startup_change_error("registry failed", None);
        assert!(startup_only.contains("registry failed"));
        assert!(!startup_only.contains("ショートカットの復元にも失敗"));
    }
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
    fn polish_preset_change_is_rejected_after_session_start() {
        assert!(ensure_polish_preset_change_allowed(true).is_err());
        assert!(ensure_polish_preset_change_allowed(false).is_ok());
    }

    #[test]
    fn saved_selected_correction_surfaces_focus_failure_as_warning() {
        assert!(selected_focus_warning(Ok(())).is_none());
        let warning = selected_focus_warning(Err(anyhow::anyhow!("focus failed"))).unwrap();
        assert!(warning.contains("保存しました"));
        assert!(!warning.contains("focus failed"));
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
            api_model: "gpt-realtime-whisper".to_string(),
            show_live_transcript_in_floating_bar: false,
            ..Default::default()
        };

        assert!(!should_start_realtime_asr(&settings, &Mode::Raw));
    }

    #[test]
    fn raw_live_transcript_on_uses_realtime_asr() {
        let settings = AppSettings {
            api_key: "sk-test".to_string(),
            api_model: "gpt-realtime-whisper".to_string(),
            show_live_transcript_in_floating_bar: true,
            ..Default::default()
        };

        assert!(should_start_realtime_asr(&settings, &Mode::Raw));
    }

    #[test]
    fn polish_live_transcript_off_keeps_realtime_asr() {
        let settings = AppSettings {
            api_key: "sk-test".to_string(),
            api_model: "gpt-realtime-whisper".to_string(),
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
