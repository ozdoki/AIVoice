use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::state::Mode;

const STORE_PATH: &str = "settings.json";
const KEYRING_SERVICE: &str = "aivoice";
const KEYRING_USER: &str = "api_key";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyBinding {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Default for HotkeyBinding {
    fn default() -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            key: String::new(),
        }
    }
}

impl HotkeyBinding {
    pub fn push_to_talk_default() -> Self {
        Self {
            ctrl: true,
            shift: true,
            key: "F4".to_string(),
            ..Default::default()
        }
    }

    pub fn hands_free_default() -> Self {
        Self {
            ctrl: true,
            shift: true,
            key: "F6".to_string(),
            ..Default::default()
        }
    }

    pub fn toggle_mode_default() -> Self {
        Self {
            ctrl: true,
            shift: true,
            key: "F5".to_string(),
            ..Default::default()
        }
    }

    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(self.key.clone());
        parts.join(" + ")
    }
}

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
    pub custom_polish_instructions: String,
    pub deep_context_enabled: bool,
    pub show_floating_bar: bool,
    pub launch_at_login: bool,
    pub push_to_talk_hotkey: HotkeyBinding,
    pub hands_free_hotkey: HotkeyBinding,
    pub toggle_mode_hotkey: HotkeyBinding,
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
            custom_polish_instructions: String::new(),
            deep_context_enabled: false,
            show_floating_bar: true,
            launch_at_login: false,
            push_to_talk_hotkey: HotkeyBinding::push_to_talk_default(),
            hands_free_hotkey: HotkeyBinding::hands_free_default(),
            toggle_mode_hotkey: HotkeyBinding::toggle_mode_default(),
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

fn save_api_key(key: &str) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    if key.is_empty() {
        let _ = entry.delete_credential();
    } else {
        entry.set_password(key)?;
    }
    Ok(())
}

/// APIキーを Credential Manager に保存する。
pub fn store_api_key(key: &str) -> anyhow::Result<()> {
    let key = key.trim();
    if key.is_empty() {
        anyhow::bail!("APIキーが空です。");
    }
    save_api_key(key)
}

/// APIキーを Credential Manager から削除する。
pub fn delete_api_key() -> anyhow::Result<()> {
    save_api_key("")
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
            custom_polish_instructions: "Slackでは短めにする".to_string(),
            deep_context_enabled: true,
            show_floating_bar: false,
            launch_at_login: true,
            push_to_talk_hotkey: HotkeyBinding::push_to_talk_default(),
            hands_free_hotkey: HotkeyBinding::hands_free_default(),
            toggle_mode_hotkey: HotkeyBinding::toggle_mode_default(),
        };
        let json = serde_json::to_value(&original).unwrap();
        let restored: AppSettings = serde_json::from_value(json).unwrap();

        assert_eq!(restored.api_base_url, original.api_base_url);
        assert_eq!(restored.api_model, original.api_model);
        assert_eq!(restored.polish_model, original.polish_model);
        assert_eq!(restored.device_id, original.device_id);
        assert_eq!(restored.mode, original.mode);
        assert_eq!(
            restored.custom_polish_instructions,
            original.custom_polish_instructions
        );
        assert_eq!(restored.deep_context_enabled, original.deep_context_enabled);
        assert_eq!(restored.show_floating_bar, original.show_floating_bar);
        assert_eq!(restored.launch_at_login, original.launch_at_login);
        assert_eq!(restored.push_to_talk_hotkey, original.push_to_talk_hotkey);
        assert_eq!(restored.hands_free_hotkey, original.hands_free_hotkey);
        assert_eq!(restored.toggle_mode_hotkey, original.toggle_mode_hotkey);
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
        assert!(d.custom_polish_instructions.is_empty());
        assert!(!d.deep_context_enabled);
        assert!(d.show_floating_bar);
        assert!(!d.launch_at_login);
        assert_eq!(d.push_to_talk_hotkey.display(), "Ctrl + Shift + F4");
        assert_eq!(d.hands_free_hotkey.display(), "Ctrl + Shift + F6");
        assert_eq!(d.toggle_mode_hotkey.display(), "Ctrl + Shift + F5");
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
        assert_eq!(s.push_to_talk_hotkey, HotkeyBinding::push_to_talk_default());
        assert_eq!(s.hands_free_hotkey, HotkeyBinding::hands_free_default());
        assert_eq!(s.toggle_mode_hotkey, HotkeyBinding::toggle_mode_default());
    }

    #[test]
    fn api_key_not_in_json() {
        let settings = AppSettings {
            api_key: "sk-secret".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert!(
            json.get("api_key").is_none(),
            "api_key should not appear in JSON"
        );
    }
}
