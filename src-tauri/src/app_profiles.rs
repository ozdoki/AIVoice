use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::{
    context::{self, FocusedAppContext},
    settings::{AppSettings, LanguageMode},
    state::Mode,
};

const STORE_PATH: &str = "app_profiles.json";
pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_PROFILES: usize = 200;
pub const MAX_NAME_CHARS: usize = 80;
pub const MAX_PROCESS_CHARS: usize = 260;
pub const MAX_TITLE_PATTERN_CHARS: usize = 200;
pub const MAX_STORE_BYTES: usize = 1024 * 1024;
pub const MAX_PRIORITY_ABS: i32 = 10_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TitleMatchKind {
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TitleCondition {
    pub match_kind: TitleMatchKind,
    pub pattern: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppProfileOverrides {
    pub mode: Option<Mode>,
    pub polish_preset: Option<String>,
    pub language_mode: Option<LanguageMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppProfile {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub process_name: String,
    #[serde(default)]
    pub title_condition: Option<TitleCondition>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub overrides: AppProfileOverrides,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppProfileStore {
    pub schema_version: u32,
    pub items: Vec<AppProfile>,
}

impl Default for AppProfileStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppProfileInput {
    pub enabled: bool,
    pub name: String,
    pub process_name: String,
    #[serde(default)]
    pub title_condition: Option<TitleCondition>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub overrides: AppProfileOverrides,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMutationResult {
    pub profile: AppProfile,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffectiveSource {
    pub kind: String,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffectiveAppProfile {
    pub process_name: String,
    pub mode: Mode,
    pub mode_source: EffectiveSource,
    pub polish_preset: String,
    pub polish_preset_source: EffectiveSource,
    pub language_mode: LanguageMode,
    pub language_mode_source: EffectiveSource,
    pub matched_profile_ids: Vec<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn normalize_process_name(value: &str) -> String {
    value
        .trim()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn process_key(value: &str) -> String {
    normalize_process_name(value).to_ascii_lowercase()
}

fn title_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn has_any_override(value: &AppProfileOverrides) -> bool {
    value.mode.is_some() || value.polish_preset.is_some() || value.language_mode.is_some()
}

fn normalize_input(mut input: AppProfileInput) -> anyhow::Result<AppProfileInput> {
    input.name = input.name.trim().to_string();
    input.process_name = normalize_process_name(&input.process_name);
    if let Some(condition) = &mut input.title_condition {
        condition.pattern = condition.pattern.trim().to_string();
        if condition.pattern.is_empty() {
            input.title_condition = None;
        }
    }
    if let Some(preset) = &mut input.overrides.polish_preset {
        *preset = preset.trim().to_string();
    }
    validate_input(&input)?;
    Ok(input)
}

fn validate_input(input: &AppProfileInput) -> anyhow::Result<()> {
    if input.name.is_empty() {
        anyhow::bail!("プロファイル名が空です。");
    }
    if input.name.chars().count() > MAX_NAME_CHARS {
        anyhow::bail!("プロファイル名は最大{MAX_NAME_CHARS}文字です。");
    }
    if input.process_name.is_empty() {
        anyhow::bail!("対象プロセス名が空です。");
    }
    if input.process_name.chars().count() > MAX_PROCESS_CHARS {
        anyhow::bail!("対象プロセス名は最大{MAX_PROCESS_CHARS}文字です。");
    }
    if let Some(condition) = &input.title_condition {
        if condition.pattern.chars().count() > MAX_TITLE_PATTERN_CHARS {
            anyhow::bail!("タイトル条件は最大{MAX_TITLE_PATTERN_CHARS}文字です。");
        }
    }
    if let Some(preset) = &input.overrides.polish_preset {
        if !matches!(
            preset.as_str(),
            "slack" | "email" | "memo" | "prompt" | "technical"
        ) {
            anyhow::bail!("Polishプリセットが不正です。");
        }
    }
    if input.priority.unsigned_abs() > MAX_PRIORITY_ABS as u32 {
        anyhow::bail!("優先度は-{MAX_PRIORITY_ABS}〜{MAX_PRIORITY_ABS}で指定してください。");
    }
    if !has_any_override(&input.overrides) {
        anyhow::bail!("少なくとも1つの上書きを指定してください。");
    }
    Ok(())
}

fn overlapping_override_names(
    left: &AppProfileOverrides,
    right: &AppProfileOverrides,
) -> Vec<&'static str> {
    let mut names = Vec::new();
    if left.mode.is_some() && right.mode.is_some() {
        names.push("モード");
    }
    if left.polish_preset.is_some() && right.polish_preset.is_some() {
        names.push("Polishプリセット");
    }
    if left.language_mode.is_some() && right.language_mode.is_some() {
        names.push("言語");
    }
    names
}

pub fn conflict_warnings(
    store: &AppProfileStore,
    input: &AppProfileInput,
    exclude_id: Option<&str>,
) -> Vec<String> {
    store
        .items
        .iter()
        .filter(|item| Some(item.id.as_str()) != exclude_id && item.enabled && input.enabled)
        .filter(|item| process_key(&item.process_name) == process_key(&input.process_name))
        .filter_map(|item| {
            let fields = overlapping_override_names(&item.overrides, &input.overrides);
            (!fields.is_empty()).then(|| {
                format!(
                    "「{}」と同じプロセスを対象にしており、{}は条件・優先度・ID順で決定されます。",
                    item.name,
                    fields.join("・")
                )
            })
        })
        .collect()
}

pub fn reject_configured_api_key(api_key: &str, input: &AppProfileInput) -> anyhow::Result<()> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Ok(());
    }
    let contains = input.name.contains(api_key)
        || input.process_name.contains(api_key)
        || input
            .title_condition
            .as_ref()
            .is_some_and(|condition| condition.pattern.contains(api_key))
        || input
            .overrides
            .polish_preset
            .as_ref()
            .is_some_and(|preset| preset.contains(api_key));
    if contains {
        anyhow::bail!("秘密情報を含む内容はアプリ別プロファイルへ保存できません。");
    }
    Ok(())
}

fn validate_store(store: &AppProfileStore) -> anyhow::Result<()> {
    if store.schema_version != SCHEMA_VERSION {
        anyhow::bail!("未対応のアプリ別プロファイル形式です。");
    }
    if store.items.len() > MAX_PROFILES {
        anyhow::bail!("アプリ別プロファイルは最大{MAX_PROFILES}件です。");
    }
    let mut ids = std::collections::HashSet::new();
    for item in &store.items {
        if item.id.trim().is_empty() || !ids.insert(item.id.as_str()) {
            anyhow::bail!("プロファイルIDが空または重複しています。");
        }
        validate_input(&AppProfileInput {
            enabled: item.enabled,
            name: item.name.clone(),
            process_name: item.process_name.clone(),
            title_condition: item.title_condition.clone(),
            priority: item.priority,
            overrides: item.overrides.clone(),
        })?;
        if normalize_process_name(&item.process_name) != item.process_name {
            anyhow::bail!("対象プロセス名が正規化されていません。");
        }
    }
    if serde_json::to_vec(store)?.len() > MAX_STORE_BYTES {
        anyhow::bail!("アプリ別プロファイルストアは最大1MiBです。");
    }
    Ok(())
}

fn decode_store_parts(
    version: Option<serde_json::Value>,
    items: Option<serde_json::Value>,
) -> anyhow::Result<AppProfileStore> {
    match (version, items) {
        (None, None) => Ok(AppProfileStore::default()),
        (Some(version), Some(items)) => {
            let store = AppProfileStore {
                schema_version: serde_json::from_value(version)?,
                items: serde_json::from_value(items)?,
            };
            validate_store(&store).map_err(|error| anyhow::anyhow!("アプリ別プロファイルを読み込めません。データは上書きしていません。設定画面の全消去で復旧できます: {error}"))?;
            Ok(store)
        }
        _ => anyhow::bail!("アプリ別プロファイルが破損しています。データは上書きしていません。設定画面の全消去で復旧できます。"),
    }
}

pub fn load(app: &AppHandle) -> anyhow::Result<AppProfileStore> {
    let store = app.store(STORE_PATH)?;
    decode_store_parts(store.get("schema_version"), store.get("items"))
}

pub fn save(app: &AppHandle, data: &AppProfileStore) -> anyhow::Result<()> {
    validate_store(data)?;
    let store = app.store(STORE_PATH)?;
    let previous_version = store.get("schema_version");
    let previous_items = store.get("items");
    store.set("schema_version", serde_json::json!(data.schema_version));
    store.set("items", serde_json::to_value(&data.items)?);
    if let Err(error) = store.save() {
        match previous_version {
            Some(value) => store.set("schema_version", value),
            None => {
                store.delete("schema_version");
            }
        }
        match previous_items {
            Some(value) => store.set("items", value),
            None => {
                store.delete("items");
            }
        }
        return Err(error.into());
    }
    Ok(())
}

pub fn clear(app: &AppHandle) -> anyhow::Result<()> {
    let store = app.store(STORE_PATH)?;
    let previous_version = store.get("schema_version");
    let previous_items = store.get("items");
    store.set("schema_version", serde_json::json!(SCHEMA_VERSION));
    store.set("items", serde_json::json!([]));
    if let Err(error) = store.save() {
        match previous_version {
            Some(value) => store.set("schema_version", value),
            None => {
                store.delete("schema_version");
            }
        }
        match previous_items {
            Some(value) => store.set("items", value),
            None => {
                store.delete("items");
            }
        }
        return Err(error.into());
    }
    Ok(())
}

fn next_id(items: &[AppProfile]) -> String {
    let base = format!("profile-{}", now_millis());
    if !items.iter().any(|item| item.id == base) {
        return base;
    }
    (1_u32..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| !items.iter().any(|item| item.id == *candidate))
        .unwrap()
}

pub fn insert(
    store: &mut AppProfileStore,
    input: AppProfileInput,
) -> anyhow::Result<ProfileMutationResult> {
    if store.items.len() >= MAX_PROFILES {
        anyhow::bail!("アプリ別プロファイルは最大{MAX_PROFILES}件です。");
    }
    let input = normalize_input(input)?;
    let warnings = conflict_warnings(store, &input, None);
    let now = now_secs();
    let profile = AppProfile {
        id: next_id(&store.items),
        enabled: input.enabled,
        name: input.name,
        process_name: input.process_name,
        title_condition: input.title_condition,
        priority: input.priority,
        overrides: input.overrides,
        created_at: now,
        updated_at: now,
    };
    store.items.push(profile.clone());
    validate_store(store)?;
    Ok(ProfileMutationResult { profile, warnings })
}

pub fn update(
    store: &mut AppProfileStore,
    id: &str,
    input: AppProfileInput,
) -> anyhow::Result<ProfileMutationResult> {
    let input = normalize_input(input)?;
    let warnings = conflict_warnings(store, &input, Some(id));
    let item = store
        .items
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow::anyhow!("プロファイルが見つかりません。"))?;
    item.enabled = input.enabled;
    item.name = input.name;
    item.process_name = input.process_name;
    item.title_condition = input.title_condition;
    item.priority = input.priority;
    item.overrides = input.overrides;
    item.updated_at = now_secs();
    let profile = item.clone();
    validate_store(store)?;
    Ok(ProfileMutationResult { profile, warnings })
}

pub fn delete(store: &mut AppProfileStore, id: &str) -> anyhow::Result<()> {
    let before = store.items.len();
    store.items.retain(|item| item.id != id);
    if store.items.len() == before {
        anyhow::bail!("プロファイルが見つかりません。");
    }
    Ok(())
}

fn source(kind: &str, profile: Option<&AppProfile>) -> EffectiveSource {
    EffectiveSource {
        kind: kind.to_string(),
        profile_id: profile.map(|item| item.id.clone()),
        profile_name: profile.map(|item| item.name.clone()),
    }
}

pub fn resolve(
    store: &AppProfileStore,
    context_value: Option<&FocusedAppContext>,
    settings: &AppSettings,
    mode_override: Option<Mode>,
) -> EffectiveAppProfile {
    let process_name = context_value
        .map(|value| normalize_process_name(&value.process_name))
        .unwrap_or_default();
    let process = process_key(&process_name);
    let title = context_value
        .map(|value| title_key(&value.window_title))
        .unwrap_or_default();
    let mut matches = store
        .items
        .iter()
        .filter(|item| {
            item.enabled && !process.is_empty() && process_key(&item.process_name) == process
        })
        .filter(|item| {
            item.title_condition
                .as_ref()
                .map(|condition| title.contains(&title_key(&condition.pattern)))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .title_condition
            .is_some()
            .cmp(&left.title_condition.is_some())
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| left.id.cmp(&right.id))
    });
    let find = |predicate: fn(&AppProfileOverrides) -> bool| {
        matches
            .iter()
            .copied()
            .find(|item| predicate(&item.overrides))
    };
    let mode_profile = find(|value| value.mode.is_some());
    let preset_profile = find(|value| value.polish_preset.is_some());
    let language_profile = find(|value| value.language_mode.is_some());
    let suggested_preset = context::suggested_polish_preset(context_value, &settings.polish_preset);
    let suggestion_used = suggested_preset != settings.polish_preset;
    EffectiveAppProfile {
        process_name,
        mode: mode_override
            .clone()
            .or_else(|| mode_profile.and_then(|item| item.overrides.mode.clone()))
            .unwrap_or_else(|| settings.mode.clone()),
        mode_source: if mode_override.is_some() {
            source("hotkey_override", None)
        } else if let Some(item) = mode_profile {
            source("profile", Some(item))
        } else {
            source("global", None)
        },
        polish_preset: preset_profile
            .and_then(|item| item.overrides.polish_preset.clone())
            .unwrap_or(suggested_preset),
        polish_preset_source: if let Some(item) = preset_profile {
            source("profile", Some(item))
        } else if suggestion_used {
            source("suggestion", None)
        } else {
            source("global", None)
        },
        language_mode: language_profile
            .and_then(|item| item.overrides.language_mode)
            .unwrap_or(settings.language_mode),
        language_mode_source: language_profile
            .map(|item| source("profile", Some(item)))
            .unwrap_or_else(|| source("global", None)),
        matched_profile_ids: matches.into_iter().map(|item| item.id.clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        name: &str,
        process: &str,
        priority: i32,
        overrides: AppProfileOverrides,
    ) -> AppProfileInput {
        AppProfileInput {
            enabled: true,
            name: name.into(),
            process_name: process.into(),
            title_condition: None,
            priority,
            overrides,
        }
    }

    #[test]
    fn normalizes_windows_and_unix_paths_to_basename() {
        assert_eq!(normalize_process_name(r"C:\Apps\Code.EXE"), "Code.EXE");
        assert_eq!(normalize_process_name("/apps/code.exe"), "code.exe");
    }

    #[test]
    fn cascades_each_field_and_title_specific_wins() {
        let mut store = AppProfileStore::default();
        let base = insert(
            &mut store,
            input(
                "base",
                "CODE.exe",
                10,
                AppProfileOverrides {
                    language_mode: Some(LanguageMode::En),
                    polish_preset: Some("technical".into()),
                    ..Default::default()
                },
            ),
        )
        .unwrap()
        .profile;
        let mut title = input(
            "docs",
            r"C:\Tools\code.exe",
            0,
            AppProfileOverrides {
                mode: Some(Mode::Polish),
                ..Default::default()
            },
        );
        title.title_condition = Some(TitleCondition {
            match_kind: TitleMatchKind::Contains,
            pattern: "README".into(),
        });
        let specific = insert(&mut store, title).unwrap().profile;
        let context = FocusedAppContext {
            process_name: "code.exe".into(),
            window_title: "README.md - project".into(),
        };
        let effective = resolve(&store, Some(&context), &AppSettings::default(), None);
        assert_eq!(effective.language_mode, LanguageMode::En);
        assert_eq!(effective.mode, Mode::Polish);
        assert_eq!(effective.language_mode_source.profile_id, Some(base.id));
        assert_eq!(effective.mode_source.profile_id, Some(specific.id));
    }

    #[test]
    fn equal_specificity_uses_priority_then_id_not_updated_at() {
        let mut store = AppProfileStore::default();
        let low = insert(
            &mut store,
            input(
                "low",
                "app.exe",
                1,
                AppProfileOverrides {
                    mode: Some(Mode::Raw),
                    ..Default::default()
                },
            ),
        )
        .unwrap()
        .profile;
        let high = insert(
            &mut store,
            input(
                "high",
                "app.exe",
                2,
                AppProfileOverrides {
                    mode: Some(Mode::Polish),
                    ..Default::default()
                },
            ),
        )
        .unwrap()
        .profile;
        store
            .items
            .iter_mut()
            .find(|item| item.id == low.id)
            .unwrap()
            .updated_at = u64::MAX;
        let context = FocusedAppContext {
            process_name: "APP.EXE".into(),
            window_title: String::new(),
        };
        let effective = resolve(&store, Some(&context), &AppSettings::default(), None);
        assert_eq!(effective.mode, Mode::Polish);
        assert_eq!(effective.mode_source.profile_id, Some(high.id));
    }

    #[test]
    fn disabled_and_missing_process_fall_back_to_global() {
        let mut store = AppProfileStore::default();
        let mut disabled = input(
            "disabled",
            "app.exe",
            0,
            AppProfileOverrides {
                mode: Some(Mode::Polish),
                ..Default::default()
            },
        );
        disabled.enabled = false;
        insert(&mut store, disabled).unwrap();
        let settings = AppSettings {
            mode: Mode::Raw,
            language_mode: LanguageMode::Ja,
            ..Default::default()
        };
        let effective = resolve(&store, None, &settings, None);
        assert_eq!(effective.mode, Mode::Raw);
        assert_eq!(effective.language_mode, LanguageMode::Ja);
        assert!(effective.matched_profile_ids.is_empty());
    }

    #[test]
    fn hotkey_mode_override_is_above_profile() {
        let mut store = AppProfileStore::default();
        insert(
            &mut store,
            input(
                "polish",
                "app.exe",
                0,
                AppProfileOverrides {
                    mode: Some(Mode::Polish),
                    ..Default::default()
                },
            ),
        )
        .unwrap();
        let context = FocusedAppContext {
            process_name: "app.exe".into(),
            window_title: String::new(),
        };
        let effective = resolve(
            &store,
            Some(&context),
            &AppSettings::default(),
            Some(Mode::Raw),
        );
        assert_eq!(effective.mode, Mode::Raw);
        assert_eq!(effective.mode_source.kind, "hotkey_override");
    }

    #[test]
    fn corrupt_or_partial_store_is_not_silently_replaced() {
        assert!(decode_store_parts(Some(serde_json::json!(1)), None).is_err());
        assert!(
            decode_store_parts(Some(serde_json::json!(99)), Some(serde_json::json!([]))).is_err()
        );
    }

    #[test]
    fn crud_preserves_created_at_and_detects_conflicts() {
        let mut store = AppProfileStore::default();
        let first = insert(
            &mut store,
            input(
                "one",
                "app.exe",
                0,
                AppProfileOverrides {
                    mode: Some(Mode::Raw),
                    ..Default::default()
                },
            ),
        )
        .unwrap()
        .profile;
        let result = insert(
            &mut store,
            input(
                "two",
                "APP.EXE",
                0,
                AppProfileOverrides {
                    mode: Some(Mode::Polish),
                    ..Default::default()
                },
            ),
        )
        .unwrap();
        assert_eq!(result.warnings.len(), 1);
        let created_at = first.created_at;
        let updated = update(
            &mut store,
            &first.id,
            input(
                "renamed",
                "app.exe",
                3,
                AppProfileOverrides {
                    mode: Some(Mode::Raw),
                    ..Default::default()
                },
            ),
        )
        .unwrap()
        .profile;
        assert_eq!(updated.created_at, created_at);
        delete(&mut store, &first.id).unwrap();
        assert_eq!(store.items.len(), 1);
    }

    #[test]
    fn disjoint_contains_conditions_still_warn_for_same_override() {
        let mut store = AppProfileStore::default();
        let mut first = input(
            "docs",
            "app.exe",
            0,
            AppProfileOverrides {
                mode: Some(Mode::Raw),
                ..Default::default()
            },
        );
        first.title_condition = Some(TitleCondition {
            match_kind: TitleMatchKind::Contains,
            pattern: "docs".into(),
        });
        insert(&mut store, first).unwrap();
        let mut second = input(
            "mail",
            "app.exe",
            0,
            AppProfileOverrides {
                mode: Some(Mode::Polish),
                ..Default::default()
            },
        );
        second.title_condition = Some(TitleCondition {
            match_kind: TitleMatchKind::Contains,
            pattern: "mail".into(),
        });
        assert_eq!(conflict_warnings(&store, &second, None).len(), 1);
    }

    #[test]
    fn equal_priority_uses_lexicographically_first_id() {
        let mut store = AppProfileStore::default();
        let first = insert(
            &mut store,
            input(
                "z",
                "app.exe",
                0,
                AppProfileOverrides {
                    mode: Some(Mode::Raw),
                    ..Default::default()
                },
            ),
        )
        .unwrap()
        .profile;
        let second = insert(
            &mut store,
            input(
                "a",
                "app.exe",
                0,
                AppProfileOverrides {
                    mode: Some(Mode::Polish),
                    ..Default::default()
                },
            ),
        )
        .unwrap()
        .profile;
        store
            .items
            .iter_mut()
            .find(|item| item.id == first.id)
            .unwrap()
            .id = "z-id".into();
        store
            .items
            .iter_mut()
            .find(|item| item.id == second.id)
            .unwrap()
            .id = "a-id".into();
        let context = FocusedAppContext {
            process_name: "app.exe".into(),
            window_title: String::new(),
        };
        let effective = resolve(&store, Some(&context), &AppSettings::default(), None);
        assert_eq!(effective.mode, Mode::Polish);
        assert_eq!(effective.mode_source.profile_id.as_deref(), Some("a-id"));
    }

    #[test]
    fn profile_preset_is_above_app_suggestion_and_global() {
        let mut store = AppProfileStore::default();
        insert(
            &mut store,
            input(
                "code mail",
                "code.exe",
                0,
                AppProfileOverrides {
                    polish_preset: Some("email".into()),
                    ..Default::default()
                },
            ),
        )
        .unwrap();
        let context = FocusedAppContext {
            process_name: "code.exe".into(),
            window_title: "project".into(),
        };
        let effective = resolve(&store, Some(&context), &AppSettings::default(), None);
        assert_eq!(effective.polish_preset, "email");
        assert_eq!(effective.polish_preset_source.kind, "profile");
    }

    #[test]
    fn resolved_values_are_immutable_snapshots() {
        let mut store = AppProfileStore::default();
        insert(
            &mut store,
            input(
                "app",
                "app.exe",
                0,
                AppProfileOverrides {
                    mode: Some(Mode::Polish),
                    ..Default::default()
                },
            ),
        )
        .unwrap();
        let context = FocusedAppContext {
            process_name: "app.exe".into(),
            window_title: String::new(),
        };
        let settings = AppSettings::default();
        let snapshot = resolve(&store, Some(&context), &settings, None);
        store.items.clear();
        assert_eq!(snapshot.mode, Mode::Polish);
        assert_eq!(snapshot.matched_profile_ids.len(), 1);
    }

    #[test]
    fn configured_api_key_is_rejected_without_echoing_it() {
        let secret = "sk-example-secret-value-123456";
        let mut value = input(
            "safe",
            "app.exe",
            0,
            AppProfileOverrides {
                mode: Some(Mode::Raw),
                ..Default::default()
            },
        );
        value.title_condition = Some(TitleCondition {
            match_kind: TitleMatchKind::Contains,
            pattern: format!("document {secret}"),
        });
        let error = reject_configured_api_key(secret, &value)
            .unwrap_err()
            .to_string();
        assert!(!error.contains(secret));
    }

    #[test]
    fn serde_roundtrip_and_profile_limit_are_validated() {
        let mut store = AppProfileStore::default();
        let profile = insert(
            &mut store,
            input(
                "app",
                "app.exe",
                0,
                AppProfileOverrides {
                    mode: Some(Mode::Raw),
                    ..Default::default()
                },
            ),
        )
        .unwrap()
        .profile;
        let decoded: AppProfileStore =
            serde_json::from_slice(&serde_json::to_vec(&store).unwrap()).unwrap();
        assert_eq!(decoded, store);
        let mut oversized = AppProfileStore {
            schema_version: SCHEMA_VERSION,
            items: vec![profile; MAX_PROFILES + 1],
        };
        for (index, item) in oversized.items.iter_mut().enumerate() {
            item.id = format!("id-{index}");
        }
        assert!(validate_store(&oversized).is_err());
    }

    #[test]
    fn legacy_clipboard_profile_override_is_ignored_and_stripped_on_save() {
        let json = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "items": [{
                "id": "legacy",
                "enabled": true,
                "name": "Legacy",
                "process_name": "app.exe",
                "overrides": { "mode": "raw", "leave_result_on_clipboard": true },
                "created_at": 1,
                "updated_at": 1
            }]
        });
        let store: AppProfileStore = serde_json::from_value(json).unwrap();
        let saved = serde_json::to_value(store).unwrap();
        assert!(saved["items"][0]["overrides"]
            .get("leave_result_on_clipboard")
            .is_none());
    }

    #[test]
    fn profile_forced_polish_is_reflected_in_data_flow_summary() {
        let mut store = AppProfileStore::default();
        insert(
            &mut store,
            input(
                "polish",
                "app.exe",
                0,
                AppProfileOverrides {
                    mode: Some(Mode::Polish),
                    language_mode: Some(LanguageMode::En),
                    ..Default::default()
                },
            ),
        )
        .unwrap();
        let context = FocusedAppContext {
            process_name: "app.exe".into(),
            window_title: String::new(),
        };
        let mut settings = AppSettings {
            mode: Mode::Raw,
            api_key: "configured-for-test".into(),
            ..Default::default()
        };
        let effective = resolve(&store, Some(&context), &settings, None);
        settings.language_mode = effective.language_mode;
        let summary = crate::data_flow::summarize_with_corrections(
            &settings,
            &effective.mode,
            false,
            &crate::data_flow::CorrectionFlowState::default(),
        );
        assert!(summary.polish.is_some());
        assert!(summary.polish.unwrap().sendable);
        assert!(summary.language.contains("英語"));
    }
}
