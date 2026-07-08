use std::time::{SystemTime, UNIX_EPOCH};
use std::{cmp::Ordering, collections::HashMap};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::state::Mode;

const HISTORY_STORE: &str = "history.json";
const DICTIONARY_STORE: &str = "dictionary.json";
const SNIPPETS_STORE: &str = "snippets.json";
const USAGE_STORE: &str = "usage.json";
const MAX_HISTORY_ITEMS: usize = 200;
pub const MAX_DICTIONARY_WORDS: usize = 800;
pub const MAX_SNIPPETS: usize = 100;
const MAX_DICTIONARY_SUGGESTIONS: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HistoryStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub raw_text: String,
    pub final_text: String,
    pub mode: Mode,
    pub duration_ms: u64,
    pub created_at: u64,
    pub error: Option<String>,
    pub status: HistoryStatus,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageDaySummary {
    pub day: String,
    pub sessions: u32,
    pub words: u32,
    pub characters: u32,
    pub audio_seconds: u32,
    pub asr_cost_usd: f64,
    pub polish_cost_usd: f64,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DictionarySuggestion {
    pub word: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnippetEntry {
    pub id: String,
    pub cue: String,
    pub text: String,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct SessionMetrics {
    pub mode: Mode,
    pub raw_text: String,
    pub final_text: String,
    pub duration_ms: u64,
    pub api_model: String,
    pub polish_model: String,
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

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

pub fn day_key_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn estimate_words(text: &str) -> u32 {
    let whitespace_words = text
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .count();
    if whitespace_words > 1 {
        whitespace_words as u32
    } else {
        text.chars().filter(|ch| !ch.is_whitespace()).count() as u32
    }
}

fn estimate_tokens(text: &str) -> f64 {
    (text.chars().filter(|ch| !ch.is_whitespace()).count() as f64 / 3.0).ceil()
}

fn asr_price_per_minute(model: &str) -> f64 {
    let model = model.to_ascii_lowercase();
    match model.as_str() {
        // OpenAI API pricing checked 2026-06-24.
        "gpt-4o-mini-transcribe" => 0.003,
        "gpt-4o-transcribe" | "whisper-1" => 0.006,
        "gpt-realtime-whisper" => 0.017,
        _ => 0.006,
    }
}

fn text_model_price_per_million(model: &str) -> (f64, f64) {
    let model = model.to_ascii_lowercase();
    match model.as_str() {
        // OpenAI API pricing checked 2026-06-24. Values are standard processing USD / 1M tokens.
        "gpt-5.5" => (5.00, 30.00),
        "gpt-5.4" => (2.50, 15.00),
        "gpt-5.4-mini" => (0.75, 4.50),
        "gpt-5.4-nano" => (0.20, 1.25),
        // Legacy fallback retained for the existing default model in this app.
        "gpt-4o-mini" => (0.15, 0.60),
        _ => (0.75, 4.50),
    }
}

fn estimate_asr_cost(duration_ms: u64, model: &str) -> f64 {
    (duration_ms as f64 / 60_000.0) * asr_price_per_minute(model)
}

fn estimate_polish_cost(raw_text: &str, final_text: &str, mode: &Mode, model: &str) -> f64 {
    if !matches!(mode, Mode::Polish) {
        return 0.0;
    }
    let (input_per_million, output_per_million) = text_model_price_per_million(model);
    let input_cost = estimate_tokens(raw_text) * input_per_million / 1_000_000.0;
    let output_cost = estimate_tokens(final_text) * output_per_million / 1_000_000.0;
    input_cost + output_cost
}

pub fn make_history_entry(
    raw_text: String,
    final_text: String,
    mode: Mode,
    duration_ms: u64,
    error: Option<String>,
) -> HistoryEntry {
    let status = if error.is_some() {
        HistoryStatus::Error
    } else {
        HistoryStatus::Success
    };
    HistoryEntry {
        id: format!("hist-{}", now_millis()),
        raw_text,
        final_text,
        mode,
        duration_ms,
        created_at: now_secs(),
        error,
        status,
        pinned: false,
    }
}

pub fn load_history(app: &AppHandle) -> anyhow::Result<Vec<HistoryEntry>> {
    let store = app.store(HISTORY_STORE)?;
    match store.get("items") {
        Some(value) => Ok(serde_json::from_value(value)?),
        None => Ok(Vec::new()),
    }
}

pub fn save_history(app: &AppHandle, items: &[HistoryEntry]) -> anyhow::Result<()> {
    let store = app.store(HISTORY_STORE)?;
    store.set("items", serde_json::to_value(items)?);
    store.save()?;
    Ok(())
}

pub fn append_history(app: &AppHandle, entry: HistoryEntry) -> anyhow::Result<HistoryEntry> {
    let mut items = load_history(app)?;
    items.insert(0, entry.clone());
    items.truncate(MAX_HISTORY_ITEMS);
    save_history(app, &items)?;
    Ok(entry)
}

pub fn delete_history_item(app: &AppHandle, id: &str) -> anyhow::Result<Vec<HistoryEntry>> {
    let mut items = load_history(app)?;
    items.retain(|item| item.id != id);
    save_history(app, &items)?;
    Ok(items)
}

pub fn toggle_history_pin(app: &AppHandle, id: &str) -> anyhow::Result<Vec<HistoryEntry>> {
    let mut items = load_history(app)?;
    let Some(item) = items.iter_mut().find(|item| item.id == id) else {
        anyhow::bail!("履歴が見つかりません。");
    };
    item.pinned = !item.pinned;
    save_history(app, &items)?;
    Ok(items)
}

pub fn clear_history(app: &AppHandle) -> anyhow::Result<()> {
    save_history(app, &[])
}

pub fn load_dictionary(app: &AppHandle) -> anyhow::Result<Vec<String>> {
    let store = app.store(DICTIONARY_STORE)?;
    match store.get("words") {
        Some(value) => Ok(serde_json::from_value(value)?),
        None => Ok(Vec::new()),
    }
}

pub fn save_dictionary(app: &AppHandle, words: &[String]) -> anyhow::Result<()> {
    let store = app.store(DICTIONARY_STORE)?;
    store.set("words", serde_json::to_value(words)?);
    store.save()?;
    Ok(())
}

pub fn normalize_dictionary_word(word: &str) -> Option<String> {
    let normalized = word.trim().to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub fn add_dictionary_word(app: &AppHandle, word: &str) -> anyhow::Result<Vec<String>> {
    let Some(word) = normalize_dictionary_word(word) else {
        anyhow::bail!("辞書に追加する単語を入力してください。");
    };
    let mut words = load_dictionary(app)?;
    if words
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&word))
    {
        return Ok(words);
    }
    if words.len() >= MAX_DICTIONARY_WORDS {
        anyhow::bail!("辞書は最大 {MAX_DICTIONARY_WORDS} 語までです。");
    }
    words.push(word);
    words.sort_by_key(|item| item.to_lowercase());
    save_dictionary(app, &words)?;
    Ok(words)
}

pub fn remove_dictionary_word(app: &AppHandle, word: &str) -> anyhow::Result<Vec<String>> {
    let mut words = load_dictionary(app)?;
    words.retain(|existing| existing != word);
    save_dictionary(app, &words)?;
    Ok(words)
}

pub fn load_snippets(app: &AppHandle) -> anyhow::Result<Vec<SnippetEntry>> {
    let store = app.store(SNIPPETS_STORE)?;
    match store.get("items") {
        Some(value) => Ok(serde_json::from_value(value)?),
        None => Ok(Vec::new()),
    }
}

pub fn save_snippets(app: &AppHandle, items: &[SnippetEntry]) -> anyhow::Result<()> {
    let store = app.store(SNIPPETS_STORE)?;
    store.set("items", serde_json::to_value(items)?);
    store.save()?;
    Ok(())
}

fn normalize_snippet_cue(cue: &str) -> Option<String> {
    let normalized = cue.trim().to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_snippet_text(text: &str) -> Option<String> {
    let normalized = text.trim().to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub fn add_snippet(app: &AppHandle, cue: &str, text: &str) -> anyhow::Result<Vec<SnippetEntry>> {
    let Some(cue) = normalize_snippet_cue(cue) else {
        anyhow::bail!("スニペットの音声キューを入力してください。");
    };
    let Some(text) = normalize_snippet_text(text) else {
        anyhow::bail!("展開するテキストを入力してください。");
    };
    let mut items = load_snippets(app)?;
    if let Some(existing) = items
        .iter_mut()
        .find(|item| item.cue.eq_ignore_ascii_case(&cue))
    {
        existing.text = text;
        save_snippets(app, &items)?;
        return Ok(items);
    }
    if items.len() >= MAX_SNIPPETS {
        anyhow::bail!("スニペットは最大 {MAX_SNIPPETS} 件までです。");
    }
    items.insert(
        0,
        SnippetEntry {
            id: format!("snip-{}", now_millis()),
            cue,
            text,
            created_at: now_secs(),
        },
    );
    save_snippets(app, &items)?;
    Ok(items)
}

pub fn remove_snippet(app: &AppHandle, id: &str) -> anyhow::Result<Vec<SnippetEntry>> {
    let mut items = load_snippets(app)?;
    items.retain(|item| item.id != id);
    save_snippets(app, &items)?;
    Ok(items)
}

pub fn expand_snippets(text: &str, snippets: &[SnippetEntry]) -> String {
    let mut expanded = text.to_string();
    let mut ordered = snippets
        .iter()
        .filter(|snippet| !snippet.cue.trim().is_empty())
        .collect::<Vec<_>>();
    ordered.sort_by_key(|snippet| std::cmp::Reverse(snippet.cue.chars().count()));
    for snippet in ordered {
        expanded = expanded.replace(&snippet.cue, &snippet.text);
    }
    expanded
}

fn is_katakana(ch: char) -> bool {
    ('\u{30A0}'..='\u{30FF}').contains(&ch)
}

fn is_ascii_candidate_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | '+' | '#' | '@')
}

fn trim_candidate_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| matches!(ch, '-' | '_' | '.' | '/' | '+' | '#' | '@'))
        .to_string()
}

fn is_dictionary_candidate(token: &str) -> bool {
    let chars = token.chars().count();
    if !(2..=64).contains(&chars) {
        return false;
    }
    if token.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    let has_ascii_alpha = token.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_symbol = token
        .chars()
        .any(|ch| matches!(ch, '-' | '_' | '.' | '/' | '+' | '#' | '@'));
    let has_uppercase = token.chars().any(|ch| ch.is_ascii_uppercase());
    let is_katakana_word = token.chars().all(is_katakana) && chars >= 3;
    has_ascii_alpha || has_symbol || has_uppercase || is_katakana_word
}

fn collect_candidate_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut ascii = String::new();
    let mut katakana = String::new();

    let flush_ascii = |tokens: &mut Vec<String>, ascii: &mut String| {
        if ascii.is_empty() {
            return;
        }
        let token = trim_candidate_token(ascii);
        if is_dictionary_candidate(&token) {
            tokens.push(token);
        }
        ascii.clear();
    };
    let flush_katakana = |tokens: &mut Vec<String>, katakana: &mut String| {
        if katakana.chars().count() >= 3 {
            tokens.push(katakana.clone());
        }
        katakana.clear();
    };

    for ch in text.chars() {
        if is_ascii_candidate_char(ch) {
            flush_katakana(&mut tokens, &mut katakana);
            ascii.push(ch);
        } else if is_katakana(ch) {
            flush_ascii(&mut tokens, &mut ascii);
            katakana.push(ch);
        } else {
            flush_ascii(&mut tokens, &mut ascii);
            flush_katakana(&mut tokens, &mut katakana);
        }
    }
    flush_ascii(&mut tokens, &mut ascii);
    flush_katakana(&mut tokens, &mut katakana);
    tokens
}

pub fn dictionary_suggestions_from_history(
    history: &[HistoryEntry],
    dictionary_words: &[String],
) -> Vec<DictionarySuggestion> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for entry in history
        .iter()
        .filter(|entry| entry.status == HistoryStatus::Success)
    {
        for text in [&entry.raw_text, &entry.final_text] {
            for token in collect_candidate_tokens(text) {
                if dictionary_words
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&token))
                {
                    continue;
                }
                *counts.entry(token).or_default() += 1;
            }
        }
    }

    let mut suggestions: Vec<DictionarySuggestion> = counts
        .into_iter()
        .map(|(word, count)| DictionarySuggestion { word, count })
        .collect();
    suggestions.sort_by(|a, b| match b.count.cmp(&a.count) {
        Ordering::Equal => a.word.to_lowercase().cmp(&b.word.to_lowercase()),
        other => other,
    });
    suggestions.truncate(MAX_DICTIONARY_SUGGESTIONS);
    suggestions
}

pub fn dictionary_suggestions(app: &AppHandle) -> anyhow::Result<Vec<DictionarySuggestion>> {
    let history = load_history(app)?;
    let dictionary_words = load_dictionary(app)?;
    Ok(dictionary_suggestions_from_history(
        &history,
        &dictionary_words,
    ))
}

pub fn load_usage(app: &AppHandle) -> anyhow::Result<Vec<UsageDaySummary>> {
    let store = app.store(USAGE_STORE)?;
    match store.get("days") {
        Some(value) => Ok(serde_json::from_value(value)?),
        None => Ok(Vec::new()),
    }
}

pub fn save_usage(app: &AppHandle, days: &[UsageDaySummary]) -> anyhow::Result<()> {
    let store = app.store(USAGE_STORE)?;
    store.set("days", serde_json::to_value(days)?);
    store.save()?;
    Ok(())
}

pub fn record_usage(
    app: &AppHandle,
    metrics: SessionMetrics,
) -> anyhow::Result<Vec<UsageDaySummary>> {
    let mut days = load_usage(app)?;
    let day = day_key_from_unix(now_secs());
    let index = days.iter().position(|item| item.day == day);
    let summary = match index {
        Some(index) => &mut days[index],
        None => {
            days.push(UsageDaySummary {
                day: day.clone(),
                ..Default::default()
            });
            days.last_mut().expect("usage day just inserted")
        }
    };

    summary.sessions = summary.sessions.saturating_add(1);
    summary.words = summary
        .words
        .saturating_add(estimate_words(&metrics.final_text));
    summary.characters = summary
        .characters
        .saturating_add(metrics.final_text.chars().count() as u32);
    summary.audio_seconds = summary
        .audio_seconds
        .saturating_add(((metrics.duration_ms + 999) / 1000) as u32);
    summary.asr_cost_usd += estimate_asr_cost(metrics.duration_ms, &metrics.api_model);
    summary.polish_cost_usd += estimate_polish_cost(
        &metrics.raw_text,
        &metrics.final_text,
        &metrics.mode,
        &metrics.polish_model,
    );
    if !summary.models.contains(&metrics.api_model) {
        summary.models.push(metrics.api_model);
    }
    if matches!(metrics.mode, Mode::Polish) && !summary.models.contains(&metrics.polish_model) {
        summary.models.push(metrics.polish_model);
    }

    days.sort_by(|a, b| b.day.cmp(&a.day));
    save_usage(app, &days)?;
    Ok(days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_word_normalization_rejects_empty() {
        assert_eq!(
            normalize_dictionary_word("  Obsidian  "),
            Some("Obsidian".to_string())
        );
        assert_eq!(normalize_dictionary_word("   "), None);
    }

    #[test]
    fn unix_day_key_is_stable() {
        assert_eq!(day_key_from_unix(0), "1970-01-01");
        assert_eq!(day_key_from_unix(86_400), "1970-01-02");
    }

    #[test]
    fn history_entry_status_follows_error() {
        let entry = make_history_entry(
            "raw".to_string(),
            "final".to_string(),
            Mode::Raw,
            1000,
            Some("failed".to_string()),
        );
        assert_eq!(entry.status, HistoryStatus::Error);
    }

    #[test]
    fn cost_estimates_use_model_specific_rates() {
        let asr = estimate_asr_cost(60_000, "gpt-4o-mini-transcribe");
        assert!((asr - 0.003).abs() < f64::EPSILON);

        let polish =
            estimate_polish_cost("hello world", "Hello world.", &Mode::Polish, "gpt-5.4-mini");
        assert!(polish > 0.0);
    }

    #[test]
    fn dictionary_suggestions_prefer_history_terms_not_already_registered() {
        let history = vec![
            HistoryEntry {
                id: "1".to_string(),
                raw_text: "AIVoice と gpt-realtime-whisper をObsidianで使う".to_string(),
                final_text: "AIVoice と gpt-realtime-whisper を Obsidian で使う".to_string(),
                mode: Mode::Raw,
                duration_ms: 1000,
                created_at: 0,
                error: None,
                status: HistoryStatus::Success,
                pinned: false,
            },
            HistoryEntry {
                id: "2".to_string(),
                raw_text: "Slack と AIVoice のテスト".to_string(),
                final_text: "Slack と AIVoice のテスト".to_string(),
                mode: Mode::Polish,
                duration_ms: 1000,
                created_at: 0,
                error: None,
                status: HistoryStatus::Success,
                pinned: false,
            },
        ];
        let suggestions = dictionary_suggestions_from_history(&history, &["Slack".to_string()]);
        let words: Vec<&str> = suggestions.iter().map(|item| item.word.as_str()).collect();
        assert!(words.contains(&"AIVoice"));
        assert!(words.contains(&"gpt-realtime-whisper"));
        assert!(words.contains(&"Obsidian"));
        assert!(!words.contains(&"Slack"));
    }

    #[test]
    fn snippet_expansion_replaces_registered_cues_longest_first() {
        let snippets = vec![
            SnippetEntry {
                id: "1".to_string(),
                cue: "署名".to_string(),
                text: "山田太郎".to_string(),
                created_at: 0,
            },
            SnippetEntry {
                id: "2".to_string(),
                cue: "署名URL".to_string(),
                text: "https://example.com".to_string(),
                created_at: 0,
            },
        ];
        assert_eq!(
            expand_snippets("署名URL と 署名", &snippets),
            "https://example.com と 山田太郎"
        );
    }
}
