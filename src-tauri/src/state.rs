use std::time::Instant;

use tokio::{
    sync::{watch, Mutex},
    task::JoinHandle,
};

use crate::{audio::CapturedAudio, settings::AppSettings};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
}

pub struct AppState {
    pub mode: Mutex<Mode>,
    pub recording_state: Mutex<RecordingState>,
    pub settings: Mutex<AppSettings>,
    pub dictionary_words: Mutex<Vec<String>>,
    pub session: Mutex<Option<SessionController>>,
    pub recording_trigger: Mutex<Option<RecordingTrigger>>,
    pub session_action: Mutex<()>,
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
        }
    }
}
