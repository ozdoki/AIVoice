use serde::Serialize;

use crate::{
    settings::{AppSettings, CorrectionLearningMode, LanguageMode},
    speech::openai_compatible::batch_transcription_model,
    speech::realtime::{
        supports_realtime_language, supports_realtime_model, supports_realtime_prompt,
        REALTIME_TRANSCRIPTION_MODEL,
    },
    state::Mode,
};

#[derive(Debug, Clone, Default)]
pub struct CorrectionFlowState {
    pub total: usize,
    pub active: usize,
    pub has_vocabulary: bool,
    pub has_replacement: bool,
    pub has_style_example: bool,
    pub store_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalApiFlow {
    pub sendable: bool,
    pub destination_host: String,
    pub model: String,
    pub fallback_model: Option<String>,
    pub processing: String,
    pub sent_data: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DataProcessingSummary {
    pub capture: String,
    pub asr: ExternalApiFlow,
    pub polish: Option<ExternalApiFlow>,
    pub selected_voice_edit: ExternalApiFlow,
    pub language: String,
    pub local_storage: Vec<String>,
    pub external_retention: String,
    pub correction_learning_enabled: bool,
    pub correction_learning_status: String,
}

pub fn effective_realtime_model(settings: &AppSettings, mode: &Mode) -> Option<String> {
    if settings.api_key.trim().is_empty()
        || !(settings.show_live_transcript_in_floating_bar || !matches!(mode, Mode::Raw))
    {
        return None;
    }
    if supports_realtime_model(&settings.api_model) {
        Some(settings.api_model.clone())
    } else if settings.show_live_transcript_in_floating_bar {
        Some(REALTIME_TRANSCRIPTION_MODEL.to_string())
    } else {
        None
    }
}

fn safe_host(base_url: &str) -> String {
    reqwest::Url::parse(base_url.trim())
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "送信先を確認できません".to_string())
}

pub fn summarize(
    settings: &AppSettings,
    mode: &Mode,
    has_dictionary: bool,
) -> DataProcessingSummary {
    summarize_with_corrections(
        settings,
        mode,
        has_dictionary,
        &CorrectionFlowState::default(),
    )
}

pub fn summarize_with_corrections(
    settings: &AppSettings,
    mode: &Mode,
    has_dictionary: bool,
    corrections: &CorrectionFlowState,
) -> DataProcessingSummary {
    let host = safe_host(&settings.api_base_url);
    let realtime_model = effective_realtime_model(settings, mode);
    let sendable = !settings.api_key.trim().is_empty();
    let learning_enabled = settings.correction_learning_mode == CorrectionLearningMode::Ask;
    let has_learned_vocabulary = learning_enabled && corrections.has_vocabulary;
    let has_learned_style_example = learning_enabled && corrections.has_style_example;
    let batch_model = batch_transcription_model(&settings.api_model);
    let mut asr_data = vec!["録音した音声".to_string()];
    if has_dictionary || has_learned_vocabulary {
        let dictionary_route = if realtime_model
            .as_ref()
            .is_some_and(|model| supports_realtime_prompt(&settings.api_base_url, model))
        {
            "登録済み辞書語（Realtime prompt／Batchフォールバック）"
        } else {
            "登録済み辞書語（Batch／フォールバック時）"
        };
        asr_data.push(dictionary_route.to_string());
    }
    if settings.deep_context_enabled {
        asr_data.push("前面アプリ名とウィンドウタイトル（Batch／フォールバック時）".to_string());
    }

    let polish = matches!(mode, Mode::Polish).then(|| {
        let mut sent_data = vec!["ASRのRaw文字起こし".to_string()];
        if has_dictionary || has_learned_vocabulary {
            sent_data.push("登録済み辞書語".to_string());
        }
        if settings.deep_context_enabled {
            sent_data.push("前面アプリ名とウィンドウタイトル".to_string());
        }
        if !settings.custom_polish_instructions.trim().is_empty() {
            sent_data.push("カスタム出力指示".to_string());
        }
        if has_learned_style_example {
            sent_data.push("確認済みの文体例（最大3件）".to_string());
        }
        ExternalApiFlow {
            sendable,
            destination_host: host.clone(),
            model: settings.polish_model.clone(),
            fallback_model: None,
            processing: if sendable {
                "設定したOpenAI互換APIで文章を整形"
            } else {
                "APIキー未設定のため現在は送信不可"
            }
            .to_string(),
            sent_data,
        }
    });

    DataProcessingSummary {
        capture: "マイク音声の取得とWAV化は端末内".to_string(),
        asr: ExternalApiFlow {
            sendable,
            destination_host: host.clone(),
            model: realtime_model
                .clone()
                .unwrap_or_else(|| batch_model.clone()),
            fallback_model: realtime_model.as_ref().map(|_| batch_model),
            processing: if !sendable {
                "APIキー未設定のため現在は送信不可".to_string()
            } else if realtime_model.is_some() {
                "Realtime（失敗・空応答時はBatchへフォールバック）".to_string()
            } else {
                "Batch".to_string()
            },
            sent_data: asr_data,
        },
        polish,
        selected_voice_edit: ExternalApiFlow {
            sendable,
            destination_host: host.clone(),
            model: settings.polish_model.clone(),
            fallback_model: None,
            processing: if sendable {
                "F9選択音声編集のときだけ、専用プロンプトで編集案を生成"
            } else {
                "APIキー未設定のため現在は送信不可"
            }
            .to_string(),
            sent_data: vec![
                "選択した原文（保存しない）".to_string(),
                "音声から認識した編集指示".to_string(),
            ],
        },
        language: match settings.language_mode {
            LanguageMode::Auto => "Auto（言語ヒントを送信しない）",
            LanguageMode::Ja
                if realtime_model.as_ref().map_or(false, |model| {
                    !supports_realtime_language(&settings.api_base_url, model)
                }) =>
            {
                "日本語（Realtimeでは未送信、Batchフォールバックではja）"
            }
            LanguageMode::En
                if realtime_model.as_ref().map_or(false, |model| {
                    !supports_realtime_language(&settings.api_base_url, model)
                }) =>
            {
                "英語（Realtimeでは未送信、Batchフォールバックではen）"
            }
            LanguageMode::Ja => "日本語（ja）",
            LanguageMode::En => "英語（en）",
        }
        .to_string(),
        local_storage: vec![
            "設定・辞書・履歴・スニペット・利用量・確認済み修正例・アプリ別プロファイル（ローカル平文）".to_string(),
            "失敗・中断時の復元用音声（成功時は削除）".to_string(),
            "APIキー（Windows Credential Manager）".to_string(),
        ],
        external_retention:
            "外部API側の保存・保持期間は接続先プロバイダの契約・設定・ポリシーに依存します"
                .to_string(),
        correction_learning_enabled: learning_enabled,
        correction_learning_status: if settings.correction_learning_mode
            == CorrectionLearningMode::Off
        {
            "オフ（保存済み修正例も処理へ適用しません）".to_string()
        } else if corrections.store_error {
            "保存データを読み込めません。通常操作では上書きせず、明示的な全消去で復旧できます"
                .to_string()
        } else {
            format!(
                "保存前に確認（全{}件・有効{}件、語彙/文体例は外部APIへ送信され得ます。置換は端末内のみ）",
                corrections.total, corrections.active
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_hides_credentials_path_query_and_fragment() {
        assert_eq!(
            safe_host("https://user:secret@example.com/private?token=x#fragment"),
            "example.com"
        );
        assert_eq!(safe_host("not a url"), "送信先を確認できません");
    }

    #[test]
    fn raw_and_polish_summaries_follow_effective_settings() {
        let settings = AppSettings {
            api_key: "test".to_string(),
            language_mode: LanguageMode::En,
            deep_context_enabled: true,
            ..AppSettings::default()
        };
        let raw = summarize(&settings, &Mode::Raw, true);
        assert!(raw.polish.is_none());
        assert_eq!(raw.asr.processing, "Batch");
        assert_eq!(raw.asr.model, "gpt-4o-mini-transcribe");
        assert!(raw.asr.sent_data.iter().any(|item| item.contains("辞書")));
        assert!(raw.asr.sent_data.iter().any(|item| item.contains("前面")));
        assert_eq!(raw.language, "英語（en）");

        let polish = summarize(&settings, &Mode::Polish, false);
        assert!(polish.polish.is_some());
        assert!(polish.asr.processing.starts_with("Realtime"));
        assert_eq!(
            polish.asr.fallback_model.as_deref(),
            Some("gpt-4o-mini-transcribe")
        );
        assert!(!polish.correction_learning_enabled);
    }

    #[test]
    fn disabled_context_and_dictionary_are_not_listed() {
        let summary = summarize(&AppSettings::default(), &Mode::Polish, false);
        let polish = summary.polish.unwrap();
        assert!(!summary
            .asr
            .sent_data
            .iter()
            .any(|item| item.contains("前面")));
        assert!(!polish.sent_data.iter().any(|item| item.contains("辞書")));
    }

    #[test]
    fn missing_api_key_is_not_reported_as_active_transmission() {
        let summary = summarize(&AppSettings::default(), &Mode::Raw, false);
        assert!(!summary.asr.sendable);
        assert!(summary.asr.processing.contains("送信不可"));
    }

    #[test]
    fn custom_realtime_endpoint_reports_language_only_on_batch_fallback() {
        let settings = AppSettings {
            api_base_url: "https://compatible.example/v1".to_string(),
            api_key: "test".to_string(),
            language_mode: LanguageMode::Ja,
            ..AppSettings::default()
        };
        let summary = summarize(&settings, &Mode::Polish, false);
        assert_eq!(
            summary.language,
            "日本語（Realtimeでは未送信、Batchフォールバックではja）"
        );
    }

    #[test]
    fn enabled_correction_artifacts_report_external_and_local_routes() {
        let settings = AppSettings {
            api_key: "test".to_string(),
            correction_learning_mode: CorrectionLearningMode::Ask,
            ..AppSettings::default()
        };
        let corrections = CorrectionFlowState {
            total: 3,
            active: 3,
            has_vocabulary: true,
            has_replacement: true,
            has_style_example: true,
            store_error: false,
        };
        let summary = summarize_with_corrections(&settings, &Mode::Polish, false, &corrections);
        assert!(summary.correction_learning_enabled);
        assert!(summary
            .correction_learning_status
            .contains("全3件・有効3件"));
        assert!(summary
            .correction_learning_status
            .contains("置換は端末内のみ"));
        assert!(summary
            .asr
            .sent_data
            .iter()
            .any(|item| item.contains("辞書語")));
        let polish = summary.polish.unwrap();
        assert!(polish.sent_data.iter().any(|item| item.contains("辞書語")));
        assert!(polish.sent_data.iter().any(|item| item.contains("文体例")));
        assert!(!polish.sent_data.iter().any(|item| item.contains("置換")));
    }

    #[test]
    fn disabled_correction_learning_ignores_stale_artifact_flags() {
        let corrections = CorrectionFlowState {
            total: 3,
            active: 3,
            has_vocabulary: true,
            has_replacement: true,
            has_style_example: true,
            store_error: false,
        };
        let summary =
            summarize_with_corrections(&AppSettings::default(), &Mode::Polish, false, &corrections);
        assert!(!summary.correction_learning_enabled);
        assert!(summary.correction_learning_status.starts_with("オフ"));
        assert!(!summary
            .asr
            .sent_data
            .iter()
            .any(|item| item.contains("辞書語")));
        assert!(!summary
            .polish
            .unwrap()
            .sent_data
            .iter()
            .any(|item| item.contains("文体例")));
    }
}
