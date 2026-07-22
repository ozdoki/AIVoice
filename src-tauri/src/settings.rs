use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::state::Mode;

const STORE_PATH: &str = "settings.json";
const KEYRING_SERVICE: &str = "aivoice";
const KEYRING_USER: &str = "api_key";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageMode {
    #[default]
    Auto,
    Ja,
    En,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CorrectionLearningMode {
    #[default]
    Off,
    Ask,
}

impl LanguageMode {
    pub fn api_language(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Ja => Some("ja"),
            Self::En => Some("en"),
        }
    }
}

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
        Self::hands_free_raw_default()
    }

    pub fn hands_free_raw_default() -> Self {
        Self {
            ctrl: true,
            shift: true,
            key: "F6".to_string(),
            ..Default::default()
        }
    }

    pub fn hands_free_polish_default() -> Self {
        Self {
            ctrl: true,
            shift: true,
            key: "F7".to_string(),
            ..Default::default()
        }
    }

    pub fn learn_selected_default() -> Self {
        Self {
            ctrl: true,
            shift: true,
            key: "F8".to_string(),
            ..Default::default()
        }
    }

    pub fn voice_edit_selected_default() -> Self {
        Self {
            ctrl: true,
            shift: true,
            key: "F9".to_string(),
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
    pub language_mode: LanguageMode,
    pub polish_model: String,
    pub mode: Mode,
    pub device_id: Option<String>,
    pub polish_preset: String,
    pub custom_polish_instructions: String,
    pub deep_context_enabled: bool,
    pub show_floating_bar: bool,
    pub show_live_transcript_in_floating_bar: bool,
    pub correction_learning_mode: CorrectionLearningMode,
    pub correction_learning_multi_diff_enabled: bool,
    pub launch_at_login: bool,
    pub onboarding_completed: bool,
    pub push_to_talk_hotkey: HotkeyBinding,
    pub hands_free_raw_hotkey: HotkeyBinding,
    pub hands_free_polish_hotkey: HotkeyBinding,
    pub learn_selected_hotkey: HotkeyBinding,
    pub voice_edit_selected_hotkey: HotkeyBinding,
    #[serde(skip_serializing, default)]
    pub hands_free_hotkey: HotkeyBinding,
    #[serde(skip_serializing, default)]
    pub toggle_mode_hotkey: HotkeyBinding,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            api_model: "gpt-realtime-whisper".to_string(),
            language_mode: LanguageMode::Auto,
            polish_model: "gpt-4o-mini".to_string(),
            mode: Mode::default(),
            device_id: None,
            polish_preset: "memo".to_string(),
            custom_polish_instructions: String::new(),
            deep_context_enabled: false,
            show_floating_bar: true,
            show_live_transcript_in_floating_bar: false,
            correction_learning_mode: CorrectionLearningMode::Off,
            correction_learning_multi_diff_enabled: false,
            launch_at_login: false,
            onboarding_completed: false,
            push_to_talk_hotkey: HotkeyBinding::push_to_talk_default(),
            hands_free_hotkey: HotkeyBinding::hands_free_default(),
            hands_free_raw_hotkey: HotkeyBinding::hands_free_raw_default(),
            hands_free_polish_hotkey: HotkeyBinding::hands_free_polish_default(),
            learn_selected_hotkey: HotkeyBinding::learn_selected_default(),
            voice_edit_selected_hotkey: HotkeyBinding::voice_edit_selected_default(),
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
        Some(v) => {
            let legacy_hands_free = v
                .get("hands_free_hotkey")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok());
            let raw_missing = v.get("hands_free_raw_hotkey").is_none();
            let mut settings: AppSettings = serde_json::from_value(v)?;
            if raw_missing {
                if let Some(legacy) = legacy_hands_free {
                    settings.hands_free_raw_hotkey = legacy;
                }
            }
            settings
        }
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
            api_model: "gpt-realtime-whisper".to_string(),
            polish_model: "gpt-4o".to_string(),
            mode: Mode::Polish,
            device_id: Some("dev-001".to_string()),
            polish_preset: "slack".to_string(),
            custom_polish_instructions: "Slackでは短めにする".to_string(),
            deep_context_enabled: true,
            show_floating_bar: false,
            show_live_transcript_in_floating_bar: true,
            launch_at_login: true,
            onboarding_completed: true,
            push_to_talk_hotkey: HotkeyBinding::push_to_talk_default(),
            hands_free_raw_hotkey: HotkeyBinding::hands_free_raw_default(),
            hands_free_polish_hotkey: HotkeyBinding::hands_free_polish_default(),
            learn_selected_hotkey: HotkeyBinding::learn_selected_default(),
            voice_edit_selected_hotkey: HotkeyBinding::voice_edit_selected_default(),
            ..Default::default()
        };
        let json = serde_json::to_value(&original).unwrap();
        let restored: AppSettings = serde_json::from_value(json).unwrap();

        assert_eq!(restored.api_base_url, original.api_base_url);
        assert_eq!(restored.api_model, original.api_model);
        assert_eq!(restored.language_mode, original.language_mode);
        assert_eq!(restored.polish_model, original.polish_model);
        assert_eq!(restored.device_id, original.device_id);
        assert_eq!(restored.mode, original.mode);
        assert_eq!(restored.polish_preset, original.polish_preset);
        assert_eq!(
            restored.custom_polish_instructions,
            original.custom_polish_instructions
        );
        assert_eq!(restored.deep_context_enabled, original.deep_context_enabled);
        assert_eq!(restored.show_floating_bar, original.show_floating_bar);
        assert_eq!(
            restored.show_live_transcript_in_floating_bar,
            original.show_live_transcript_in_floating_bar
        );
        assert_eq!(restored.launch_at_login, original.launch_at_login);
        assert_eq!(restored.onboarding_completed, original.onboarding_completed);
        assert_eq!(restored.push_to_talk_hotkey, original.push_to_talk_hotkey);
        assert_eq!(
            restored.hands_free_raw_hotkey,
            original.hands_free_raw_hotkey
        );
        assert_eq!(
            restored.hands_free_polish_hotkey,
            original.hands_free_polish_hotkey
        );
        assert_eq!(
            restored.learn_selected_hotkey,
            original.learn_selected_hotkey
        );
        assert_eq!(
            restored.voice_edit_selected_hotkey,
            original.voice_edit_selected_hotkey
        );
        // api_key は serde(skip) のため JSON 経由では復元されない
        assert!(restored.api_key.is_empty());
    }

    #[test]
    fn settings_default_values() {
        let d = AppSettings::default();
        assert!(d.api_key.is_empty());
        assert_eq!(d.api_base_url, "https://api.openai.com/v1");
        assert_eq!(d.api_model, "gpt-realtime-whisper");
        assert_eq!(d.language_mode, LanguageMode::Auto);
        assert!(d.device_id.is_none());
        assert_eq!(d.mode, Mode::Raw);
        assert_eq!(d.polish_preset, "memo");
        assert!(d.custom_polish_instructions.is_empty());
        assert!(!d.deep_context_enabled);
        assert!(d.show_floating_bar);
        assert!(!d.show_live_transcript_in_floating_bar);
        assert!(!d.launch_at_login);
        assert!(!d.onboarding_completed);
        assert_eq!(d.push_to_talk_hotkey.display(), "Ctrl + Shift + F4");
        assert_eq!(d.hands_free_raw_hotkey.display(), "Ctrl + Shift + F6");
        assert_eq!(d.hands_free_polish_hotkey.display(), "Ctrl + Shift + F7");
        assert_eq!(d.learn_selected_hotkey.display(), "Ctrl + Shift + F8");
        assert_eq!(d.voice_edit_selected_hotkey.display(), "Ctrl + Shift + F9");
    }

    #[test]
    fn settings_missing_device_id_uses_none() {
        // device_id が JSON にない場合、#[serde(default)] で None になること
        let json = serde_json::json!({
            "api_base_url": "https://api.openai.com/v1",
            "api_model": "gpt-4o-mini-transcribe",
            "polish_model": "gpt-4o-mini",
            "mode": "raw"
        });
        let s: AppSettings = serde_json::from_value(json).unwrap();
        assert!(s.device_id.is_none());
        assert_eq!(s.language_mode, LanguageMode::Auto);
        assert_eq!(s.push_to_talk_hotkey, HotkeyBinding::push_to_talk_default());
        assert_eq!(
            s.hands_free_raw_hotkey,
            HotkeyBinding::hands_free_raw_default()
        );
        assert_eq!(
            s.hands_free_polish_hotkey,
            HotkeyBinding::hands_free_polish_default()
        );
        assert_eq!(
            s.learn_selected_hotkey,
            HotkeyBinding::learn_selected_default()
        );
        assert_eq!(
            s.voice_edit_selected_hotkey,
            HotkeyBinding::voice_edit_selected_default()
        );
    }

    #[test]
    fn settings_missing_polish_preset_uses_default() {
        let json = serde_json::json!({
            "api_base_url": "https://api.openai.com/v1",
            "api_model": "gpt-4o-mini-transcribe",
            "polish_model": "gpt-4o-mini",
            "mode": "polish"
        });
        let s: AppSettings = serde_json::from_value(json).unwrap();
        assert_eq!(s.polish_preset, "memo");
    }

    #[test]
    fn legacy_clipboard_setting_is_ignored_and_stripped_on_save() {
        let mut json = serde_json::to_value(AppSettings::default()).unwrap();
        json["leave_result_on_clipboard"] = serde_json::json!(true);
        let settings: AppSettings = serde_json::from_value(json).unwrap();
        let saved = serde_json::to_value(settings).unwrap();
        assert!(saved.get("leave_result_on_clipboard").is_none());
    }

    #[test]
    fn language_mode_serde_uses_stable_values() {
        for (mode, value) in [
            (LanguageMode::Auto, "auto"),
            (LanguageMode::Ja, "ja"),
            (LanguageMode::En, "en"),
        ] {
            assert_eq!(serde_json::to_value(mode).unwrap(), value);
            assert_eq!(
                serde_json::from_value::<LanguageMode>(serde_json::json!(value)).unwrap(),
                mode
            );
        }
    }

    #[test]
    fn correction_learning_mode_has_only_off_and_ask_stable_values() {
        assert_eq!(
            serde_json::to_value(CorrectionLearningMode::Off).unwrap(),
            "off"
        );
        assert_eq!(
            serde_json::to_value(CorrectionLearningMode::Ask).unwrap(),
            "ask"
        );
        assert!(serde_json::from_str::<CorrectionLearningMode>("\"auto\"").is_err());
        assert_eq!(
            AppSettings::default().correction_learning_mode,
            CorrectionLearningMode::Off
        );
    }

    #[test]
    fn multiline_custom_polish_instructions_survive_serde_roundtrip() {
        let settings = AppSettings {
            custom_polish_instructions:
                "# 整形の粒度\n- 句読点は自然な呼吸位置に。\n- 段落は意味の切れ目で入れる。"
                    .to_string(),
            ..AppSettings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        let restored: AppSettings = serde_json::from_value(json).unwrap();
        assert_eq!(
            restored.custom_polish_instructions,
            settings.custom_polish_instructions
        );
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
