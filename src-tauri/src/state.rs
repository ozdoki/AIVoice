use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::Instant,
};

use tokio::{
    sync::{watch, Mutex},
    task::JoinHandle,
};

use crate::{
    audio::CapturedAudio,
    context::FocusedWindowTarget,
    corrections::CorrectionSessionSnapshot,
    selected_learning::PendingSelectedLearning,
    selected_voice_edit::{ActiveSelectedVoiceEdit, PendingSelectedVoiceEdit},
    settings::{AppSettings, LanguageMode},
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Raw,
    Polish,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Raw
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingState {
    Idle,
    Recording,
    Processing,
}

impl Default for RecordingState {
    fn default() -> Self {
        RecordingState::Idle
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecordingTrigger {
    PushToTalk,
    HandsFree,
    Manual,
}

/// アクティブな録音セッションの制御ハンドル。
pub struct SessionController {
    pub stop_tx: watch::Sender<bool>,
    pub capture_task: JoinHandle<anyhow::Result<CapturedAudio>>,
    pub realtime_task: Option<JoinHandle<Result<String, String>>>,
    pub started_at: Instant,
    pub recovery_id: Option<String>,
    pub mode: Mode,
    pub polish_preset: String,
    pub app_process: String,
    pub correction_snapshot: CorrectionSessionSnapshot,
    pub language_mode: LanguageMode,
    pub target_window: Option<FocusedWindowTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Dictation,
    SelectedVoiceEdit,
}

const CANCELLATION_ACCEPTING: u8 = 0;
const CANCELLATION_REQUESTED: u8 = 1;
const PROCESSING_COMMITTED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingGateOutcome {
    CancelRequested,
    ProcessingCommitted,
}

pub struct AppState {
    pub mode: Mutex<Mode>,
    pub recording_state: Mutex<RecordingState>,
    pub settings: Mutex<AppSettings>,
    pub dictionary_words: Mutex<Vec<String>>,
    pub session: Mutex<Option<SessionController>>,
    pub recording_trigger: Mutex<Option<RecordingTrigger>>,
    pub session_action: Mutex<()>,
    /// history.json のread-modify-writeを直列化する。
    pub history_action: Mutex<()>,
    pub session_kind: Mutex<Option<SessionKind>>,
    /// Escapeと通常停止が同じCASを競う単一ゲート。
    cancellation_gate: AtomicU8,
    pub last_target_window: Mutex<Option<FocusedWindowTarget>>,
    /// corrections.json のread-modify-writeを直列化する。
    pub corrections_action: Mutex<()>,
    /// 選択テキスト学習のcapture・pending更新・確認を直列化する。
    pub selected_learning_action: Mutex<()>,
    /// 本文を永続化せず、単一の確認操作だけを最大10分保持する。
    pub pending_selected_learning: Mutex<Option<PendingSelectedLearning>>,
    pub active_selected_voice_edit: Mutex<Option<ActiveSelectedVoiceEdit>>,
    pub pending_selected_voice_edit: Mutex<Option<PendingSelectedVoiceEdit>>,
    /// app_profiles.json のread-modify-writeを直列化する。
    pub app_profiles_action: Mutex<()>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: Mutex::new(Mode::default()),
            recording_state: Mutex::new(RecordingState::default()),
            settings: Mutex::new(AppSettings::default()),
            dictionary_words: Mutex::new(Vec::new()),
            session: Mutex::new(None),
            recording_trigger: Mutex::new(None),
            session_action: Mutex::new(()),
            history_action: Mutex::new(()),
            session_kind: Mutex::new(None),
            cancellation_gate: AtomicU8::new(CANCELLATION_ACCEPTING),
            last_target_window: Mutex::new(None),
            corrections_action: Mutex::new(()),
            selected_learning_action: Mutex::new(()),
            pending_selected_learning: Mutex::new(None),
            active_selected_voice_edit: Mutex::new(None),
            pending_selected_voice_edit: Mutex::new(None),
            app_profiles_action: Mutex::new(()),
        }
    }
}

impl AppState {
    /// Processing commitより先にAcceptingを取得できたEscapeだけを受理する。
    pub fn request_cancellation(&self) -> bool {
        self.cancellation_gate
            .compare_exchange(
                CANCELLATION_ACCEPTING,
                CANCELLATION_REQUESTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// Escapeと停止のうち、先にCASした側を確定する。
    pub fn commit_processing_or_observe_cancel(&self) -> ProcessingGateOutcome {
        match self.cancellation_gate.compare_exchange(
            CANCELLATION_ACCEPTING,
            PROCESSING_COMMITTED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) | Err(PROCESSING_COMMITTED) => ProcessingGateOutcome::ProcessingCommitted,
            Err(CANCELLATION_REQUESTED) => ProcessingGateOutcome::CancelRequested,
            Err(other) => panic!("invalid cancellation gate state: {other}"),
        }
    }

    pub fn reset_cancellation_gate(&self) {
        self.cancellation_gate
            .store(CANCELLATION_ACCEPTING, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_wins_when_it_transitions_first() {
        let state = AppState::default();
        assert!(state.request_cancellation());
        assert!(!state.request_cancellation());
        assert_eq!(
            state.commit_processing_or_observe_cancel(),
            ProcessingGateOutcome::CancelRequested
        );
    }

    #[test]
    fn processing_commit_rejects_late_escape_without_latching() {
        let state = AppState::default();
        assert_eq!(
            state.commit_processing_or_observe_cancel(),
            ProcessingGateOutcome::ProcessingCommitted
        );
        assert!(!state.request_cancellation());
        assert_eq!(
            state.commit_processing_or_observe_cancel(),
            ProcessingGateOutcome::ProcessingCommitted
        );
        state.reset_cancellation_gate();
        assert!(state.request_cancellation());
    }

    #[test]
    fn terminal_reset_clears_every_prior_gate_state() {
        let state = AppState::default();
        assert!(state.request_cancellation());
        state.reset_cancellation_gate();
        assert_eq!(
            state.commit_processing_or_observe_cancel(),
            ProcessingGateOutcome::ProcessingCommitted
        );
        assert!(!state.request_cancellation());
        state.reset_cancellation_gate();
        assert!(state.request_cancellation());
    }
}
