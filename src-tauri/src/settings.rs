use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::state::Mode;

const STORE_PATH: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub api_base_url: String,
    pub api_key: String,
    pub api_model: String,
    pub polish_model: String,
    pub mode: Mode,
    pub device_id: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            api_model: "whisper-1".to_string(),
            polish_model: "gpt-4o-mini".to_string(),
            mode: Mode::default(),
            device_id: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Mode;

    #[test]
    fn settings_roundtrip() {
        let original = AppSettings {
            api_base_url: "https://example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            api_model: "whisper-1".to_string(),
            polish_model: "gpt-4o".to_string(),
            mode: Mode::Polish,
            device_id: Some("dev-001".to_string()),
        };
        let json = serde_json::to_value(&original).unwrap();
        let restored: AppSettings = serde_json::from_value(json).unwrap();

        assert_eq!(restored.api_base_url, original.api_base_url);
        assert_eq!(restored.api_key, original.api_key);
        assert_eq!(restored.api_model, original.api_model);
        assert_eq!(restored.polish_model, original.polish_model);
        assert_eq!(restored.device_id, original.device_id);
        assert_eq!(restored.mode, original.mode);
    }

    #[test]
    fn settings_default_values() {
        let d = AppSettings::default();
        assert!(d.api_key.is_empty());
        assert_eq!(d.api_base_url, "https://api.openai.com/v1");
        assert_eq!(d.api_model, "whisper-1");
        assert!(d.device_id.is_none());
        assert_eq!(d.mode, Mode::Raw);
    }

    #[test]
    fn settings_missing_device_id_uses_none() {
        // device_id が JSON にない場合、#[serde(default)] で None になること
        let json = serde_json::json!({
            "api_base_url": "https://api.openai.com/v1",
            "api_key": "",
            "api_model": "whisper-1",
            "polish_model": "gpt-4o-mini",
            "mode": "raw"
        });
        let s: AppSettings = serde_json::from_value(json).unwrap();
        assert!(s.device_id.is_none());
    }
}
