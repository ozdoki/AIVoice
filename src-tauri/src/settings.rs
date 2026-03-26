use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::state::Mode;

const STORE_PATH: &str = "settings.json";
const KEYRING_SERVICE: &str = "aivoice";
const KEYRING_USER: &str = "api_key";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub api_base_url: String,
    /// api_key は JSON には保存しない。Credential Manager で管理する。
    /// skip_serializing のみ: ストア保存時は出力しないが、invoke 受信時はデシリアライズする。
    #[serde(skip_serializing, default)]
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

/// Credential Manager から api_key を読み込む。
/// 未登録・エラー時は空文字を返す。
fn load_api_key() -> String {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .and_then(|e| e.get_password())
        .unwrap_or_default()
}

/// api_key を Credential Manager に保存する。
/// 空文字が渡された場合はエントリを削除する。
fn save_api_key(key: &str) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    if key.is_empty() {
        let _ = entry.delete_credential();
    } else {
        entry.set_password(key)?;
    }
    Ok(())
}

pub fn load(app: &AppHandle) -> anyhow::Result<AppSettings> {
    let store = app.store(STORE_PATH)?;
    let mut s: AppSettings = match store.get("settings") {
        Some(v) => serde_json::from_value(v)?,
        None => AppSettings::default(),
    };
    s.api_key = load_api_key();
    Ok(s)
}

pub fn save(app: &AppHandle, settings: &AppSettings) -> anyhow::Result<()> {
    let store = app.store(STORE_PATH)?;
    store.set("settings", serde_json::to_value(settings)?);
    store.save()?;
    save_api_key(&settings.api_key)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Mode;

    #[test]
    fn settings_roundtrip() {
        // api_key は #[serde(skip)] のため JSON には含まれない。
        // JSON 経由のフィールドのみ検証する。
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
        assert_eq!(restored.api_model, original.api_model);
        assert_eq!(restored.polish_model, original.polish_model);
        assert_eq!(restored.device_id, original.device_id);
        assert_eq!(restored.mode, original.mode);
        // api_key は serde(skip) のため JSON 経由では復元されない
        assert!(restored.api_key.is_empty());
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
            "api_model": "whisper-1",
            "polish_model": "gpt-4o-mini",
            "mode": "raw"
        });
        let s: AppSettings = serde_json::from_value(json).unwrap();
        assert!(s.device_id.is_none());
    }

    #[test]
    fn api_key_not_in_json() {
        let settings = AppSettings {
            api_key: "sk-secret".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json.get("api_key").is_none(), "api_key should not appear in JSON");
    }
}
