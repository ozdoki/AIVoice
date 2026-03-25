use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_PATH: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub api_base_url: String,
    pub api_key: String,
    pub api_model: String,
    pub polish_model: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            api_model: "whisper-1".to_string(),
            polish_model: "gpt-4o-mini".to_string(),
        }
    }
}

pub fn load(app: &AppHandle) -> anyhow::Result<AppSettings> {
    let store = app.store(STORE_PATH)?;
    match store.get("settings") {
        Some(v) => Ok(serde_json::from_value(v)?),
        None => Ok(AppSettings::default()),
    }
}

pub fn save(app: &AppHandle, settings: &AppSettings) -> anyhow::Result<()> {
    let store = app.store(STORE_PATH)?;
    store.set("settings", serde_json::to_value(settings)?);
    store.save()?;
    Ok(())
}
