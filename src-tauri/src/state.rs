use tokio::sync::Mutex;

use crate::settings::AppSettings;

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

pub struct AppState {
    pub mode: Mutex<Mode>,
    pub recording_state: Mutex<RecordingState>,
    pub settings: Mutex<AppSettings>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: Mutex::new(Mode::default()),
            recording_state: Mutex::new(RecordingState::default()),
            settings: Mutex::new(AppSettings::default()),
        }
    }
}
