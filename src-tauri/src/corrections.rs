use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::{settings::CorrectionLearningMode, state::Mode};

const CORRECTIONS_STORE: &str = "corrections.json";
pub const CORRECTIONS_SCHEMA_VERSION: u32 = 1;
pub const MAX_CORRECTIONS: usize = 500;
pub const MAX_TEXT_CHARS: usize = 20_000;
pub const MAX_ARTIFACTS_PER_CORRECTION: usize = 16;
pub const MAX_VOCABULARY_CHARS: usize = 128;
pub const MAX_REPLACEMENT_FROM_CHARS: usize = 128;
pub const MAX_REPLACEMENT_TO_CHARS: usize = 2_000;
pub const MAX_APP_PROCESS_CHARS: usize = 260;
pub const MAX_STYLE_EXAMPLE_CHARS: usize = 8_000;
pub const MAX_STORE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_EFFECTIVE_VOCABULARY: usize = 800;
pub const MAX_EFFECTIVE_VOCABULARY_CHARS: usize = 8_000;
pub const MAX_ACTIVE_REPLACEMENTS: usize = 500;
pub const MAX_STYLE_EXAMPLES: usize = 3;
pub const MAX_STYLE_EXAMPLE_TOTAL_CHARS: usize = 6_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionStatus {
    Active,
    Undone,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionClassification {
    Minor,
    Substantial,
    MeaningChangeSuspected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LearningScope {
    Global,
    App,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CorrectionArtifact {
    Vocabulary {
        value: String,
        scope: LearningScope,
    },
    Replacement {
        from: String,
        to: String,
        scope: LearningScope,
    },
    StyleExample {
        input: String,
        output: String,
    },
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorrectionRecord {
    pub id: String,
    pub source_history_id: String,
    pub raw_text: String,
    pub original_text: String,
    pub corrected_text: String,
    pub mode: Mode,
    pub polish_preset: String,
    pub app_process: String,
    pub classification: CorrectionClassification,
    pub status: CorrectionStatus,
    pub artifacts: Vec<CorrectionArtifact>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorrectionStore {
    pub schema_version: u32,
    pub items: Vec<CorrectionRecord>,
}

#[derive(Debug, Clone)]
pub struct CorrectionSessionSnapshot {
    pub learning_mode: CorrectionLearningMode,
    pub store: CorrectionStore,
    pub dictionary_words: Vec<String>,
}

impl Default for CorrectionSessionSnapshot {
    fn default() -> Self {
        Self {
            learning_mode: CorrectionLearningMode::Off,
            store: CorrectionStore::default(),
            dictionary_words: Vec::new(),
        }
    }
}

pub fn make_session_snapshot(
    learning_mode: CorrectionLearningMode,
    manual_dictionary: &[String],
    store: CorrectionStore,
    app_process: &str,
) -> CorrectionSessionSnapshot {
    let dictionary_words = if learning_mode == CorrectionLearningMode::Ask {
        effective_vocabulary(manual_dictionary, &store, app_process)
    } else {
        manual_dictionary.to_vec()
    };
    CorrectionSessionSnapshot {
        learning_mode,
        store: if learning_mode == CorrectionLearningMode::Ask {
            store
        } else {
            CorrectionStore::default()
        },
        dictionary_words,
    }
}

impl Default for CorrectionStore {
    fn default() -> Self {
        Self {
            schema_version: CORRECTIONS_SCHEMA_VERSION,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewCorrection {
    pub source_history_id: String,
    pub raw_text: String,
    pub original_text: String,
    pub corrected_text: String,
    pub mode: Mode,
    pub polish_preset: String,
    pub app_process: String,
    pub classification: CorrectionClassification,
    pub artifacts: Vec<CorrectionArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCorrection {
    pub corrected_text: String,
    pub classification: CorrectionClassification,
    pub artifacts: Vec<CorrectionArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionPreview {
    pub source_history_id: String,
    pub original_text: String,
    pub corrected_text: String,
    pub app_process: String,
    pub classification: CorrectionClassification,
    pub candidates: Vec<CorrectionArtifact>,
    /// 保守的に常にnone。候補はユーザーが明示選択した場合だけ保存する。
    pub default_artifacts: Vec<CorrectionArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleExample {
    pub input: String,
    pub output: String,
}

pub fn apply_session_snapshot(
    snapshot: &CorrectionSessionSnapshot,
    raw_text: &str,
    polish_preset: &str,
    app_process: &str,
) -> (String, Vec<String>, Vec<StyleExample>) {
    if snapshot.learning_mode == CorrectionLearningMode::Ask {
        (
            apply_replacements(raw_text, &snapshot.store, app_process),
            snapshot.dictionary_words.clone(),
            select_style_examples(&snapshot.store, polish_preset, app_process),
        )
    } else {
        (
            raw_text.to_string(),
            snapshot.dictionary_words.clone(),
            Vec::new(),
        )
    }
}

#[derive(Debug, Clone)]
struct ReplacementRule {
    from: String,
    to: String,
    app_specific: bool,
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

fn char_count(value: &str) -> usize {
    value.chars().count()
}

fn ensure_nonempty(value: &str, label: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{label}が空です。");
    }
    Ok(())
}

fn ensure_char_limit(value: &str, limit: usize, label: &str) -> anyhow::Result<()> {
    if char_count(value) > limit {
        anyhow::bail!("{label}は最大{limit}文字です。");
    }
    Ok(())
}

fn contains_likely_api_key(value: &str) -> bool {
    value
        .split(|character: char| character.is_whitespace() || matches!(character, '"' | '\''))
        .any(|part| {
            let part = part.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '-' && character != '_'
            });
            part.starts_with("sk-") && part.chars().count() >= 24
        })
}

pub fn reject_configured_api_key(
    api_key: &str,
    raw_text: &str,
    original_text: &str,
    corrected_text: &str,
    polish_preset: &str,
    app_process: &str,
    artifacts: &[CorrectionArtifact],
) -> anyhow::Result<()> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Ok(());
    }
    let contains_key = [
        raw_text,
        original_text,
        corrected_text,
        polish_preset,
        app_process,
    ]
    .into_iter()
    .any(|value| value.contains(api_key))
        || artifacts.iter().any(|artifact| match artifact {
            CorrectionArtifact::Vocabulary { value, .. } => value.contains(api_key),
            CorrectionArtifact::Replacement { from, to, .. } => {
                from.contains(api_key) || to.contains(api_key)
            }
            CorrectionArtifact::StyleExample { input, output } => {
                input.contains(api_key) || output.contains(api_key)
            }
            CorrectionArtifact::None => false,
        });
    if contains_key {
        anyhow::bail!("現在設定されているAPIキーを含む内容は学習データへ保存できません。");
    }
    Ok(())
}

fn validate_artifact(artifact: &CorrectionArtifact, app_process: &str) -> anyhow::Result<()> {
    match artifact {
        CorrectionArtifact::Vocabulary { value, scope } => {
            ensure_nonempty(value, "語彙")?;
            ensure_char_limit(value, MAX_VOCABULARY_CHARS, "語彙")?;
            if matches!(scope, LearningScope::App) && app_process.trim().is_empty() {
                anyhow::bail!("アプリ別語彙には入力先アプリが必要です。");
            }
            if contains_likely_api_key(value) {
                anyhow::bail!("APIキーらしい値は学習データへ保存できません。");
            }
        }
        CorrectionArtifact::Replacement { from, to, scope } => {
            ensure_nonempty(from, "置換元")?;
            ensure_nonempty(to, "置換先")?;
            ensure_char_limit(from, MAX_REPLACEMENT_FROM_CHARS, "置換元")?;
            ensure_char_limit(to, MAX_REPLACEMENT_TO_CHARS, "置換先")?;
            if from == to {
                anyhow::bail!("置換元と置換先が同じです。");
            }
            if overly_general_replacement_from(from) {
                anyhow::bail!("置換元が短すぎるか一般的すぎます。");
            }
            if matches!(scope, LearningScope::App) && app_process.trim().is_empty() {
                anyhow::bail!("アプリ別置換には入力先アプリが必要です。");
            }
            if contains_likely_api_key(from) || contains_likely_api_key(to) {
                anyhow::bail!("APIキーらしい値は学習データへ保存できません。");
            }
        }
        CorrectionArtifact::StyleExample { input, output } => {
            ensure_nonempty(input, "文体例の入力")?;
            ensure_nonempty(output, "文体例の出力")?;
            if input == output {
                anyhow::bail!("文体例の入力と出力が同じです。");
            }
            if char_count(input).saturating_add(char_count(output)) > MAX_STYLE_EXAMPLE_CHARS {
                anyhow::bail!("文体例は入出力合計で最大{MAX_STYLE_EXAMPLE_CHARS}文字です。");
            }
            if contains_likely_api_key(input) || contains_likely_api_key(output) {
                anyhow::bail!("APIキーらしい値は学習データへ保存できません。");
            }
        }
        CorrectionArtifact::None => {}
    }
    Ok(())
}

fn normalized_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn levenshtein_distance(left: &[char], right: &[char]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_char) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right.iter().enumerate() {
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + usize::from(left_char != right_char));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

pub fn classify(original: &str, corrected: &str) -> CorrectionClassification {
    let left = original.chars().collect::<Vec<_>>();
    let right = corrected.chars().collect::<Vec<_>>();
    let longest = left.len().max(right.len()).max(1);
    let shortest = left.len().min(right.len()).max(1);
    let similarity = 1.0 - levenshtein_distance(&left, &right) as f64 / longest as f64;
    let length_ratio = longest as f64 / shortest as f64;
    if similarity < 0.55 || length_ratio > 2.0 {
        CorrectionClassification::MeaningChangeSuspected
    } else if similarity < 0.82 {
        CorrectionClassification::Substantial
    } else {
        CorrectionClassification::Minor
    }
}

fn single_diff(original: &str, corrected: &str) -> Option<(String, String)> {
    let left = original.chars().collect::<Vec<_>>();
    let right = corrected.chars().collect::<Vec<_>>();
    let mut prefix = 0;
    while prefix < left.len() && prefix < right.len() && left[prefix] == right[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < left.len().saturating_sub(prefix)
        && suffix < right.len().saturating_sub(prefix)
        && left[left.len() - 1 - suffix] == right[right.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let from = left[prefix..left.len() - suffix].iter().collect::<String>();
    let to = right[prefix..right.len() - suffix]
        .iter()
        .collect::<String>();
    if from.is_empty() || to.is_empty() {
        None
    } else {
        Some((from, to))
    }
}

fn overly_general_replacement_from(value: &str) -> bool {
    let normalized = value.trim().to_lowercase();
    normalized.chars().count() < 2
        || matches!(
            normalized.as_str(),
            "です" | "ます" | "する" | "これ" | "それ" | "the" | "to" | "of" | "in"
        )
}

pub fn preview(
    source_history_id: String,
    original_text: String,
    corrected_text: String,
    mode: &Mode,
    app_process: String,
) -> anyhow::Result<CorrectionPreview> {
    ensure_nonempty(&original_text, "元の出力")?;
    ensure_nonempty(&corrected_text, "修正後テキスト")?;
    ensure_char_limit(&original_text, MAX_TEXT_CHARS, "元の出力")?;
    ensure_char_limit(&corrected_text, MAX_TEXT_CHARS, "修正後テキスト")?;
    if original_text == corrected_text {
        anyhow::bail!("元の出力と修正後テキストが同じです。");
    }
    let classification = classify(&original_text, &corrected_text);
    let mut candidates = Vec::new();
    if !matches!(
        classification,
        CorrectionClassification::MeaningChangeSuspected
    ) {
        if let Some((from, to)) = single_diff(&original_text, &corrected_text) {
            if !overly_general_replacement_from(&from)
                && char_count(&from) <= MAX_REPLACEMENT_FROM_CHARS
                && char_count(&to) <= MAX_REPLACEMENT_TO_CHARS
            {
                candidates.push(CorrectionArtifact::Replacement {
                    from,
                    to,
                    scope: LearningScope::Global,
                });
            }
        }
        if matches!(mode, Mode::Polish) {
            candidates.push(CorrectionArtifact::StyleExample {
                input: original_text.clone(),
                output: corrected_text.clone(),
            });
        }
    }
    Ok(CorrectionPreview {
        source_history_id,
        original_text,
        corrected_text,
        app_process,
        classification,
        candidates,
        default_artifacts: vec![CorrectionArtifact::None],
    })
}

pub fn effective_vocabulary(
    manual: &[String],
    store: &CorrectionStore,
    app_process: &str,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut chars = 0_usize;
    let candidates = manual.iter().cloned().chain(
        store
            .items
            .iter()
            .filter(|record| record.status == CorrectionStatus::Active)
            .flat_map(|record| {
                record.artifacts.iter().filter_map(move |artifact| {
                    let CorrectionArtifact::Vocabulary { value, scope } = artifact else {
                        return None;
                    };
                    let applies = matches!(scope, LearningScope::Global)
                        || record.app_process.eq_ignore_ascii_case(app_process);
                    applies.then(|| value.clone())
                })
            }),
    );
    for candidate in candidates {
        let key = normalized_key(&candidate);
        let candidate_chars = char_count(&candidate);
        if key.is_empty()
            || seen.contains(&key)
            || result.len() >= MAX_EFFECTIVE_VOCABULARY
            || chars.saturating_add(candidate_chars) > MAX_EFFECTIVE_VOCABULARY_CHARS
        {
            continue;
        }
        seen.insert(key);
        chars += candidate_chars;
        result.push(candidate);
    }
    result
}

fn replacement_rules(store: &CorrectionStore, app_process: &str) -> Vec<ReplacementRule> {
    let mut rules = store
        .items
        .iter()
        .filter(|record| record.status == CorrectionStatus::Active)
        .flat_map(|record| {
            record.artifacts.iter().filter_map(move |artifact| {
                let CorrectionArtifact::Replacement { from, to, scope } = artifact else {
                    return None;
                };
                let app_specific = matches!(scope, LearningScope::App);
                let applies = !app_specific || record.app_process.eq_ignore_ascii_case(app_process);
                applies.then(|| ReplacementRule {
                    from: from.clone(),
                    to: to.clone(),
                    app_specific,
                })
            })
        })
        .take(MAX_ACTIVE_REPLACEMENTS)
        .collect::<Vec<_>>();
    rules.sort_by(|left, right| {
        right
            .app_specific
            .cmp(&left.app_specific)
            .then_with(|| char_count(&right.from).cmp(&char_count(&left.from)))
    });
    rules
}

/// 左から入力だけを1回走査する。生成した置換先は再走査しない。
pub fn apply_replacements(text: &str, store: &CorrectionStore, app_process: &str) -> String {
    let rules = replacement_rules(store, app_process);
    if rules.is_empty() {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut byte_index = 0_usize;
    while byte_index < text.len() {
        let remaining = &text[byte_index..];
        if let Some(rule) = rules.iter().find(|rule| remaining.starts_with(&rule.from)) {
            result.push_str(&rule.to);
            byte_index += rule.from.len();
        } else {
            let character = remaining
                .chars()
                .next()
                .expect("remaining text is non-empty");
            result.push(character);
            byte_index += character.len_utf8();
        }
    }
    result
}

pub fn select_style_examples(
    store: &CorrectionStore,
    polish_preset: &str,
    app_process: &str,
) -> Vec<StyleExample> {
    let mut examples = store
        .items
        .iter()
        .filter(|record| {
            record.status == CorrectionStatus::Active
                && record.mode == Mode::Polish
                && record.polish_preset == polish_preset
                && (record.app_process.is_empty()
                    || record.app_process.eq_ignore_ascii_case(app_process))
        })
        .flat_map(|record| {
            record.artifacts.iter().filter_map(move |artifact| {
                let CorrectionArtifact::StyleExample { input, output } = artifact else {
                    return None;
                };
                Some((
                    record.app_process.eq_ignore_ascii_case(app_process)
                        && !record.app_process.is_empty(),
                    record.updated_at,
                    StyleExample {
                        input: input.clone(),
                        output: output.clone(),
                    },
                ))
            })
        })
        .collect::<Vec<_>>();
    examples.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let mut total_chars = 0_usize;
    examples
        .into_iter()
        .filter_map(|(_, _, example)| {
            let chars = char_count(&example.input).saturating_add(char_count(&example.output));
            if total_chars.saturating_add(chars) > MAX_STYLE_EXAMPLE_TOTAL_CHARS {
                return None;
            }
            total_chars += chars;
            Some(example)
        })
        .take(MAX_STYLE_EXAMPLES)
        .collect()
}

pub fn validate_record(record: &CorrectionRecord) -> anyhow::Result<()> {
    ensure_nonempty(&record.id, "修正ID")?;
    ensure_nonempty(&record.source_history_id, "履歴ID")?;
    ensure_nonempty(&record.original_text, "元の出力")?;
    ensure_nonempty(&record.corrected_text, "修正後テキスト")?;
    ensure_char_limit(&record.raw_text, MAX_TEXT_CHARS, "Rawテキスト")?;
    ensure_char_limit(&record.original_text, MAX_TEXT_CHARS, "元の出力")?;
    ensure_char_limit(&record.corrected_text, MAX_TEXT_CHARS, "修正後テキスト")?;
    ensure_char_limit(&record.app_process, MAX_APP_PROCESS_CHARS, "入力先アプリ")?;
    if record.original_text == record.corrected_text {
        anyhow::bail!("元の出力と修正後テキストが同じです。");
    }
    if record.artifacts.len() > MAX_ARTIFACTS_PER_CORRECTION {
        anyhow::bail!("学習候補は1件につき最大{MAX_ARTIFACTS_PER_CORRECTION}件です。");
    }
    if record.artifacts.is_empty() {
        anyhow::bail!("学習候補を選択してください。適用しない場合はnoneを指定してください。");
    }
    let none_count = record
        .artifacts
        .iter()
        .filter(|artifact| matches!(artifact, CorrectionArtifact::None))
        .count();
    if none_count > 0 && (record.artifacts.len() != 1 || none_count != 1) {
        anyhow::bail!("noneは他の学習候補と同時に指定できません。");
    }
    if [
        &record.raw_text,
        &record.original_text,
        &record.corrected_text,
        &record.polish_preset,
        &record.app_process,
    ]
    .into_iter()
    .any(|value| contains_likely_api_key(value))
    {
        anyhow::bail!("APIキーらしい値は学習データへ保存できません。");
    }
    for artifact in &record.artifacts {
        if matches!(artifact, CorrectionArtifact::StyleExample { .. })
            && record.mode != Mode::Polish
        {
            anyhow::bail!("Polish文体例はPolish履歴にだけ保存できます。");
        }
        validate_artifact(artifact, &record.app_process)?;
    }
    Ok(())
}

fn validate_replacement_graph(store: &CorrectionStore) -> anyhow::Result<()> {
    let mut mappings = std::collections::HashMap::<(String, String), String>::new();
    for record in store
        .items
        .iter()
        .filter(|record| record.status == CorrectionStatus::Active)
    {
        for artifact in &record.artifacts {
            let CorrectionArtifact::Replacement { from, to, scope } = artifact else {
                continue;
            };
            let scope_key = match scope {
                LearningScope::Global => "global".to_string(),
                LearningScope::App => format!("app:{}", record.app_process.to_lowercase()),
            };
            let key = (scope_key.clone(), from.clone());
            if let Some(existing) = mappings.get(&key) {
                if existing != to {
                    anyhow::bail!("同じ範囲・置換元に競合する置換があります。");
                }
            } else {
                mappings.insert(key, to.clone());
            }
        }
    }
    for ((scope, start), _) in mappings.iter() {
        let mut seen = HashSet::new();
        let mut current = start.clone();
        while let Some(next) = mappings.get(&(scope.clone(), current.clone())) {
            if !seen.insert(current.clone()) {
                anyhow::bail!("置換ルールに循環があります。");
            }
            current = next.clone();
        }
    }
    Ok(())
}

pub fn validate_store(store: &CorrectionStore) -> anyhow::Result<()> {
    if store.schema_version != CORRECTIONS_SCHEMA_VERSION {
        anyhow::bail!(
            "未対応の修正学習ストア版です（{}）。全消去で復旧できます。",
            store.schema_version
        );
    }
    if store.items.len() > MAX_CORRECTIONS {
        anyhow::bail!("修正学習は最大{MAX_CORRECTIONS}件です。");
    }
    let mut ids = HashSet::new();
    let mut source_and_text = HashSet::new();
    for record in &store.items {
        validate_record(record)?;
        if !ids.insert(record.id.clone()) {
            anyhow::bail!("修正学習ストアに重複IDがあります。");
        }
        if !source_and_text.insert((
            record.source_history_id.clone(),
            normalized_key(&record.corrected_text),
        )) {
            anyhow::bail!("同じ履歴と修正内容が重複しています。");
        }
    }
    validate_replacement_graph(store)?;
    let bytes = serde_json::to_vec(store)?.len();
    if bytes > MAX_STORE_BYTES {
        anyhow::bail!("修正学習ストアは最大5MiBです。");
    }
    Ok(())
}

pub fn load(app: &AppHandle) -> anyhow::Result<CorrectionStore> {
    let store = app.store(CORRECTIONS_STORE)?;
    load_from_store(store.as_ref())
}

fn load_from_store<R: tauri::Runtime>(
    store: &tauri_plugin_store::Store<R>,
) -> anyhow::Result<CorrectionStore> {
    decode_store_parts(store.get("schema_version"), store.get("items"))
}

fn decode_store_parts(
    version: Option<serde_json::Value>,
    items: Option<serde_json::Value>,
) -> anyhow::Result<CorrectionStore> {
    match (version, items) {
        (None, None) => Ok(CorrectionStore::default()),
        (Some(version), Some(items)) => {
            let data = CorrectionStore {
                schema_version: serde_json::from_value(version).map_err(|error| {
                    anyhow::anyhow!(
                        "修正学習データを読み込めません。データは上書きしていません。設定画面の全消去で復旧できます: {error}"
                    )
                })?,
                items: serde_json::from_value(items).map_err(|error| {
                    anyhow::anyhow!(
                        "修正学習データを読み込めません。データは上書きしていません。設定画面の全消去で復旧できます: {error}"
                    )
                })?,
            };
            validate_store(&data).map_err(|error| {
                anyhow::anyhow!(
                    "修正学習データを読み込めません。データは上書きしていません。設定画面の全消去で復旧できます: {error}"
                )
            })?;
            Ok(data)
        }
        _ => anyhow::bail!(
            "修正学習データが破損しています。データは上書きしていません。設定画面の全消去で復旧できます。"
        ),
    }
}

pub fn save(app: &AppHandle, data: &CorrectionStore) -> anyhow::Result<()> {
    let store = app.store(CORRECTIONS_STORE)?;
    save_to_store(store.as_ref(), data)
}

fn save_to_store<R: tauri::Runtime>(
    store: &tauri_plugin_store::Store<R>,
    data: &CorrectionStore,
) -> anyhow::Result<()> {
    validate_store(data)?;
    store.set("schema_version", serde_json::json!(data.schema_version));
    store.set("items", serde_json::to_value(&data.items)?);
    store.save()?;
    Ok(())
}

/// ユーザーが明示的に選んだ場合だけ、破損storeも空のv1として上書きする復旧経路。
pub fn clear(app: &AppHandle) -> anyhow::Result<()> {
    let store = app.store(CORRECTIONS_STORE)?;
    clear_store(store.as_ref())
}

fn clear_store<R: tauri::Runtime>(store: &tauri_plugin_store::Store<R>) -> anyhow::Result<()> {
    store.set(
        "schema_version",
        serde_json::json!(CORRECTIONS_SCHEMA_VERSION),
    );
    store.set("items", serde_json::json!([]));
    store.save()?;
    Ok(())
}

fn next_id(items: &[CorrectionRecord]) -> String {
    let base = format!("corr-{}", now_millis());
    if !items.iter().any(|item| item.id == base) {
        return base;
    }
    let mut suffix = 1_u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !items.iter().any(|item| item.id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

pub fn insert(
    data: &mut CorrectionStore,
    input: NewCorrection,
) -> anyhow::Result<CorrectionRecord> {
    if data.items.len() >= MAX_CORRECTIONS {
        anyhow::bail!("修正学習は最大{MAX_CORRECTIONS}件です。");
    }
    let now = now_secs();
    let record = CorrectionRecord {
        id: next_id(&data.items),
        source_history_id: input.source_history_id,
        raw_text: input.raw_text,
        original_text: input.original_text,
        corrected_text: input.corrected_text,
        mode: input.mode,
        polish_preset: input.polish_preset,
        app_process: input.app_process,
        classification: input.classification,
        status: CorrectionStatus::Active,
        artifacts: input.artifacts,
        created_at: now,
        updated_at: now,
    };
    validate_record(&record)?;
    if data.items.iter().any(|existing| {
        existing.source_history_id == record.source_history_id
            && normalized_key(&existing.corrected_text) == normalized_key(&record.corrected_text)
    }) {
        anyhow::bail!("同じ履歴と修正内容は既に保存されています。");
    }
    data.items.insert(0, record.clone());
    validate_store(data)?;
    Ok(record)
}

pub fn update(
    data: &mut CorrectionStore,
    id: &str,
    input: UpdateCorrection,
) -> anyhow::Result<CorrectionRecord> {
    let index = data
        .items
        .iter()
        .position(|item| item.id == id)
        .ok_or_else(|| anyhow::anyhow!("修正学習データが見つかりません。"))?;
    let mut updated = data.items[index].clone();
    updated.corrected_text = input.corrected_text;
    updated.classification = input.classification;
    updated.artifacts = input.artifacts;
    updated.updated_at = now_secs();
    validate_record(&updated)?;
    if data
        .items
        .iter()
        .enumerate()
        .any(|(other_index, existing)| {
            other_index != index
                && existing.source_history_id == updated.source_history_id
                && normalized_key(&existing.corrected_text)
                    == normalized_key(&updated.corrected_text)
        })
    {
        anyhow::bail!("同じ履歴と修正内容は既に保存されています。");
    }
    data.items[index] = updated.clone();
    validate_store(data)?;
    Ok(updated)
}

pub fn set_status(
    data: &mut CorrectionStore,
    id: &str,
    status: CorrectionStatus,
) -> anyhow::Result<CorrectionRecord> {
    let item = data
        .items
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow::anyhow!("修正学習データが見つかりません。"))?;
    item.status = status;
    item.updated_at = now_secs();
    Ok(item.clone())
}

pub fn delete(data: &mut CorrectionStore, id: &str) -> anyhow::Result<()> {
    let previous_len = data.items.len();
    data.items.retain(|item| item.id != id);
    if data.items.len() == previous_len {
        anyhow::bail!("修正学習データが見つかりません。");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_store_roundtrip_clear_and_corrupt_read_preserves_bytes() {
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_store::Builder::default().build())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let path = std::env::temp_dir().join(format!(
            "koetype-corrections-{}-{}.json",
            std::process::id(),
            now_millis()
        ));
        let store = tauri_plugin_store::StoreBuilder::new(app.handle(), &path)
            .build()
            .unwrap();
        let mut data = CorrectionStore::default();
        insert(&mut data, sample_input()).unwrap();
        save_to_store(store.as_ref(), &data).unwrap();
        assert_eq!(load_from_store(store.as_ref()).unwrap().items.len(), 1);

        clear_store(store.as_ref()).unwrap();
        drop(store);
        let store = tauri_plugin_store::StoreBuilder::new(app.handle(), &path)
            .build()
            .unwrap();
        assert!(
            load_from_store(store.as_ref()).unwrap().items.is_empty(),
            "clear must survive a new store instance reload"
        );

        store.set("schema_version", serde_json::json!("corrupt"));
        store.set("items", serde_json::json!([]));
        store.save().unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(load_from_store(store.as_ref()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        drop(store);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    fn sample_input() -> NewCorrection {
        NewCorrection {
            source_history_id: "hist-1".to_string(),
            raw_text: "Koe Typeを開く".to_string(),
            original_text: "Koe Typeを開く".to_string(),
            corrected_text: "KoeTypeを開く".to_string(),
            mode: Mode::Raw,
            polish_preset: "memo".to_string(),
            app_process: "notepad.exe".to_string(),
            classification: CorrectionClassification::Minor,
            artifacts: vec![CorrectionArtifact::Replacement {
                from: "Koe Type".to_string(),
                to: "KoeType".to_string(),
                scope: LearningScope::Global,
            }],
        }
    }

    #[test]
    fn schema_and_tagged_artifacts_roundtrip() {
        let mut store = CorrectionStore::default();
        insert(&mut store, sample_input()).unwrap();
        let json = serde_json::to_value(&store).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["items"][0]["artifacts"][0]["type"], "replacement");
        assert_eq!(
            serde_json::from_value::<CorrectionStore>(json).unwrap(),
            store
        );
    }

    #[test]
    fn crud_and_status_are_persistent_model_operations() {
        let mut store = CorrectionStore::default();
        let created = insert(&mut store, sample_input()).unwrap();
        assert_eq!(created.status, CorrectionStatus::Active);
        assert_eq!(
            set_status(&mut store, &created.id, CorrectionStatus::Undone)
                .unwrap()
                .status,
            CorrectionStatus::Undone
        );
        let updated = update(
            &mut store,
            &created.id,
            UpdateCorrection {
                corrected_text: "KoeTypeを起動する".to_string(),
                classification: CorrectionClassification::Substantial,
                artifacts: vec![CorrectionArtifact::None],
            },
        )
        .unwrap();
        assert_eq!(updated.corrected_text, "KoeTypeを起動する");
        delete(&mut store, &created.id).unwrap();
        assert!(store.items.is_empty());
    }

    #[test]
    fn rejects_empty_same_duplicate_secret_and_limits() {
        let mut store = CorrectionStore::default();
        let created = insert(&mut store, sample_input()).unwrap();
        assert!(insert(&mut store, sample_input()).is_err());

        let mut same = sample_input();
        same.source_history_id = "hist-2".to_string();
        same.corrected_text = same.original_text.clone();
        assert!(insert(&mut store, same).is_err());

        let mut too_long = sample_input();
        too_long.source_history_id = "hist-3".to_string();
        too_long.corrected_text = "あ".repeat(MAX_TEXT_CHARS + 1);
        assert!(insert(&mut store, too_long).is_err());

        let mut secret = sample_input();
        secret.source_history_id = "hist-4".to_string();
        secret.corrected_text = "sk-abcdefghijklmnopqrstuvwxyz".to_string();
        assert!(insert(&mut store, secret).is_err());
        assert!(!created.id.is_empty());
    }

    #[test]
    fn configured_provider_secret_is_rejected_without_format_assumptions() {
        let artifacts = vec![CorrectionArtifact::Vocabulary {
            value: "prefix-provider-secret-123-suffix".to_string(),
            scope: LearningScope::Global,
        }];
        let error = reject_configured_api_key(
            "provider-secret-123",
            "raw",
            "original",
            "corrected",
            "memo",
            "app.exe",
            &artifacts,
        )
        .unwrap_err()
        .to_string();
        assert!(!error.contains("provider-secret-123"));
        assert!(reject_configured_api_key(
            "",
            "raw",
            "original",
            "corrected",
            "memo",
            "app.exe",
            &artifacts,
        )
        .is_ok());
    }

    #[test]
    fn rejects_unknown_schema_and_partial_none() {
        let future = CorrectionStore {
            schema_version: 99,
            items: Vec::new(),
        };
        assert!(validate_store(&future).is_err());

        let mut input = sample_input();
        input.artifacts.push(CorrectionArtifact::None);
        assert!(insert(&mut CorrectionStore::default(), input).is_err());
    }

    #[test]
    fn unicode_limits_count_scalars_not_bytes() {
        let value = "界".repeat(MAX_VOCABULARY_CHARS);
        assert!(ensure_char_limit(&value, MAX_VOCABULARY_CHARS, "語彙").is_ok());
        assert!(ensure_char_limit(&(value + "界"), MAX_VOCABULARY_CHARS, "語彙").is_err());
    }

    #[test]
    fn replacement_is_app_first_longest_and_single_pass() {
        let mut store = CorrectionStore::default();
        let mut global = sample_input();
        global.artifacts = vec![CorrectionArtifact::Replacement {
            from: "New York".to_string(),
            to: "NY".to_string(),
            scope: LearningScope::Global,
        }];
        insert(&mut store, global).unwrap();
        let mut app = sample_input();
        app.source_history_id = "hist-2".to_string();
        app.corrected_text = "app correction".to_string();
        app.artifacts = vec![CorrectionArtifact::Replacement {
            from: "New".to_string(),
            to: "New York".to_string(),
            scope: LearningScope::App,
        }];
        insert(&mut store, app).unwrap();
        assert_eq!(
            apply_replacements("New York", &store, "notepad.exe"),
            "New York York"
        );
        assert_eq!(apply_replacements("New York", &store, "other.exe"), "NY");
    }

    #[test]
    fn undo_reactivate_and_delete_change_the_next_pipeline_read_immediately() {
        let mut store = CorrectionStore::default();
        let created = insert(&mut store, sample_input()).unwrap();
        assert_eq!(
            apply_replacements("Koe Type", &store, "notepad.exe"),
            "KoeType"
        );
        set_status(&mut store, &created.id, CorrectionStatus::Undone).unwrap();
        assert_eq!(
            apply_replacements("Koe Type", &store, "notepad.exe"),
            "Koe Type"
        );
        set_status(&mut store, &created.id, CorrectionStatus::Active).unwrap();
        assert_eq!(
            apply_replacements("Koe Type", &store, "notepad.exe"),
            "KoeType"
        );
        delete(&mut store, &created.id).unwrap();
        assert_eq!(
            apply_replacements("Koe Type", &store, "notepad.exe"),
            "Koe Type"
        );
    }

    #[test]
    fn replacement_conflict_and_cycle_are_rejected() {
        let mut store = CorrectionStore::default();
        let mut first = sample_input();
        first.artifacts = vec![CorrectionArtifact::Replacement {
            from: "alpha".to_string(),
            to: "beta".to_string(),
            scope: LearningScope::Global,
        }];
        insert(&mut store, first).unwrap();

        let mut conflict = sample_input();
        conflict.source_history_id = "hist-2".to_string();
        conflict.corrected_text = "conflict".to_string();
        conflict.artifacts = vec![CorrectionArtifact::Replacement {
            from: "alpha".to_string(),
            to: "gamma".to_string(),
            scope: LearningScope::Global,
        }];
        assert!(insert(&mut store, conflict).is_err());

        let mut cycle = sample_input();
        cycle.source_history_id = "hist-3".to_string();
        cycle.corrected_text = "cycle".to_string();
        cycle.artifacts = vec![CorrectionArtifact::Replacement {
            from: "beta".to_string(),
            to: "alpha".to_string(),
            scope: LearningScope::Global,
        }];
        assert!(insert(&mut store, cycle).is_err());
    }

    #[test]
    fn vocabulary_union_keeps_manual_first_and_deduplicates_mixed_language() {
        let mut store = CorrectionStore::default();
        let mut learned = sample_input();
        learned.artifacts = vec![
            CorrectionArtifact::Vocabulary {
                value: "KoeType".to_string(),
                scope: LearningScope::Global,
            },
            CorrectionArtifact::Vocabulary {
                value: "音声入力".to_string(),
                scope: LearningScope::Global,
            },
        ];
        insert(&mut store, learned).unwrap();
        assert_eq!(
            effective_vocabulary(&["koetype".to_string()], &store, "notepad.exe"),
            vec!["koetype".to_string(), "音声入力".to_string()]
        );
    }

    #[test]
    fn session_snapshot_freezes_ask_generation_and_off_discards_store() {
        let mut store = CorrectionStore::default();
        let mut learned = sample_input();
        learned.artifacts = vec![CorrectionArtifact::Vocabulary {
            value: "LearnedTerm".to_string(),
            scope: LearningScope::Global,
        }];
        let created = insert(&mut store, learned).unwrap();
        let ask = make_session_snapshot(
            CorrectionLearningMode::Ask,
            &["Manual".to_string()],
            store.clone(),
            "notepad.exe",
        );
        set_status(&mut store, &created.id, CorrectionStatus::Undone).unwrap();
        assert_eq!(ask.dictionary_words, vec!["Manual", "LearnedTerm"]);
        assert_eq!(ask.store.items[0].status, CorrectionStatus::Active);

        let off = make_session_snapshot(
            CorrectionLearningMode::Off,
            &["Manual".to_string()],
            store,
            "notepad.exe",
        );
        assert_eq!(off.dictionary_words, vec!["Manual"]);
        assert!(off.store.items.is_empty());
    }

    #[test]
    fn stop_pipeline_snapshot_applies_replacement_vocabulary_and_few_shot_only_when_active() {
        let mut store = CorrectionStore::default();
        let mut learned = sample_input();
        learned.mode = Mode::Polish;
        learned.artifacts = vec![
            CorrectionArtifact::Vocabulary {
                value: "KoeType".into(),
                scope: LearningScope::Global,
            },
            CorrectionArtifact::Replacement {
                from: "Koe Type".into(),
                to: "KoeType".into(),
                scope: LearningScope::Global,
            },
            CorrectionArtifact::StyleExample {
                input: "rough memo".into(),
                output: "Polished memo.".into(),
            },
        ];
        let created = insert(&mut store, learned).unwrap();
        let ask = make_session_snapshot(
            CorrectionLearningMode::Ask,
            &["Manual".into()],
            store.clone(),
            "notepad.exe",
        );
        let (text, dictionary, examples) =
            apply_session_snapshot(&ask, "Koe Type", "memo", "notepad.exe");
        assert_eq!(text, "KoeType");
        assert_eq!(dictionary, vec!["Manual", "KoeType"]);
        assert_eq!(examples.len(), 1);

        set_status(&mut store, &created.id, CorrectionStatus::Undone).unwrap();
        let undone = make_session_snapshot(
            CorrectionLearningMode::Ask,
            &["Manual".into()],
            store.clone(),
            "notepad.exe",
        );
        let (text, dictionary, examples) =
            apply_session_snapshot(&undone, "Koe Type", "memo", "notepad.exe");
        assert_eq!(text, "Koe Type");
        assert_eq!(dictionary, vec!["Manual"]);
        assert!(examples.is_empty());

        let off = make_session_snapshot(
            CorrectionLearningMode::Off,
            &["Manual".into()],
            store,
            "notepad.exe",
        );
        let (text, dictionary, examples) =
            apply_session_snapshot(&off, "Koe Type", "memo", "notepad.exe");
        assert_eq!(text, "Koe Type");
        assert_eq!(dictionary, vec!["Manual"]);
        assert!(examples.is_empty());
    }

    #[test]
    fn style_examples_prefer_same_app_then_newest_and_cap_count() {
        let mut store = CorrectionStore::default();
        for index in 0..4 {
            let mut input = sample_input();
            input.source_history_id = format!("hist-style-{index}");
            input.corrected_text = format!("corrected-{index}");
            input.mode = Mode::Polish;
            input.artifacts = vec![CorrectionArtifact::StyleExample {
                input: format!("input-{index}"),
                output: format!("output-{index}"),
            }];
            if index == 0 {
                input.app_process = String::new();
            }
            insert(&mut store, input).unwrap();
            store.items[0].updated_at = index;
        }
        let examples = select_style_examples(&store, "memo", "notepad.exe");
        assert_eq!(examples.len(), 3);
        assert_eq!(examples[0].input, "input-3");
        assert!(examples.iter().all(|example| example.input != "input-0"));
    }

    #[test]
    fn preview_is_conservative_for_large_or_meaning_changes() {
        let result = preview(
            "hist-1".to_string(),
            "短いメモ".to_string(),
            "Completely unrelated long English instruction with another meaning".to_string(),
            &Mode::Polish,
            String::new(),
        )
        .unwrap();
        assert_eq!(
            result.classification,
            CorrectionClassification::MeaningChangeSuspected
        );
        assert!(result.candidates.is_empty());
        assert_eq!(result.default_artifacts, vec![CorrectionArtifact::None]);
    }

    #[test]
    fn preview_classification_corpus_covers_languages_and_edge_inputs() {
        let cases = [
            (
                "今日は会議です",
                "今日は会議です。",
                CorrectionClassification::Minor,
            ),
            (
                "The meeting starts now",
                "The meeting starts now.",
                CorrectionClassification::Minor,
            ),
            (
                "Koe Typeをopenする",
                "KoeTypeをopenする",
                CorrectionClassification::Minor,
            ),
            ("。", "、", CorrectionClassification::MeaningChangeSuspected),
            (
                "短い予定メモ",
                "Completely unrelated English paragraph about another project and deadline",
                CorrectionClassification::MeaningChangeSuspected,
            ),
        ];
        for (index, (original, corrected, expected)) in cases.into_iter().enumerate() {
            let result = preview(
                format!("corpus-{index}"),
                original.into(),
                corrected.into(),
                &Mode::Polish,
                "notepad.exe".into(),
            )
            .unwrap();
            assert_eq!(result.classification, expected, "case {index}");
        }
        assert!(preview(
            "whitespace".into(),
            "   \n".into(),
            "text".into(),
            &Mode::Raw,
            String::new(),
        )
        .is_err());
    }

    #[test]
    fn corrupt_or_partial_store_is_an_error_and_missing_store_is_empty() {
        assert_eq!(
            decode_store_parts(None, None).unwrap(),
            CorrectionStore::default()
        );
        assert!(decode_store_parts(Some(serde_json::json!(1)), None).is_err());
        assert!(decode_store_parts(
            Some(serde_json::json!("not-a-version")),
            Some(serde_json::json!([]))
        )
        .is_err());
        assert!(decode_store_parts(
            Some(serde_json::json!(1)),
            Some(serde_json::json!({ "not": "items" }))
        )
        .is_err());
    }

    #[test]
    fn serialized_store_byte_limit_is_independent_from_character_limits() {
        let mut store = CorrectionStore::default();
        for index in 0..300 {
            let now = now_secs();
            store.items.push(CorrectionRecord {
                id: format!("corr-large-{index}"),
                source_history_id: format!("hist-large-{index}"),
                raw_text: String::new(),
                original_text: "original".to_string(),
                corrected_text: format!("{}-{index}", "界".repeat(7_000)),
                mode: Mode::Raw,
                polish_preset: "memo".to_string(),
                app_process: String::new(),
                classification: CorrectionClassification::Substantial,
                status: CorrectionStatus::Active,
                artifacts: vec![CorrectionArtifact::None],
                created_at: now,
                updated_at: now,
            });
        }
        assert!(serde_json::to_vec(&store).unwrap().len() > MAX_STORE_BYTES);
        assert!(validate_store(&store).is_err());
    }
}
