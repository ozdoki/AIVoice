use crate::{
    context::FocusedAppContext,
    corrections::StyleExample,
    polish::{self, PolishState},
    settings::AppSettings,
    state::Mode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeRouteOutcome {
    pub text: String,
    pub polish_state: PolishState,
}

fn applied_state(original: &str, polished: &str) -> PolishState {
    if polished == original {
        PolishState::AppliedUnchanged
    } else {
        PolishState::AppliedChanged
    }
}

/// モードに応じてテキストを加工する。
/// Raw: そのまま返す。Polish: LLM による文章整形。
pub async fn route(
    mode: &Mode,
    settings: &AppSettings,
    dictionary_words: &[String],
    focused_context: Option<&FocusedAppContext>,
    text: &str,
) -> ModeRouteOutcome {
    route_with_style_examples(mode, settings, dictionary_words, focused_context, &[], text).await
}

pub async fn route_with_style_examples(
    mode: &Mode,
    settings: &AppSettings,
    dictionary_words: &[String],
    focused_context: Option<&FocusedAppContext>,
    style_examples: &[StyleExample],
    text: &str,
) -> ModeRouteOutcome {
    match mode {
        Mode::Raw => ModeRouteOutcome {
            text: text.to_string(),
            polish_state: PolishState::NotRequested,
        },
        Mode::Polish => {
            match polish::polish_text_with_examples(
                settings,
                dictionary_words,
                focused_context,
                style_examples,
                text,
            )
            .await
            {
                Ok(polished) => {
                    let polish_state = applied_state(text, &polished);
                    ModeRouteOutcome {
                        text: polished,
                        polish_state,
                    }
                }
                Err(error) => {
                    let polish_state = error.state();
                    tracing::warn!(
                        polish_state = ?polish_state,
                        "polish_text failed, falling back to raw: {error}"
                    );
                    ModeRouteOutcome {
                        text: text.to_string(),
                        polish_state,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn raw_mode_returns_text_unchanged() {
        let result = route(&Mode::Raw, &AppSettings::default(), &[], None, "テスト入力").await;
        assert_eq!(result.text, "テスト入力");
        assert_eq!(result.polish_state, PolishState::NotRequested);
    }

    #[tokio::test]
    async fn raw_mode_empty_string() {
        let result = route(&Mode::Raw, &AppSettings::default(), &[], None, "").await;
        assert_eq!(result.text, "");
    }

    /// Polish モードで api_key が空の場合、raw にフォールバックして理由を保持する。
    #[tokio::test]
    async fn polish_mode_falls_back_to_raw_on_api_error() {
        let settings = AppSettings::default(); // api_key 空 → API 呼び出し失敗
        let result = route(&Mode::Polish, &settings, &[], None, "元の文").await;
        assert_eq!(result.text, "元の文");
        assert_eq!(result.polish_state, PolishState::FallbackNotConfigured);
    }

    #[tokio::test]
    async fn raw_mode_language_corpus_is_never_rewritten_or_translated() {
        for text in [
            "今日はKoeTypeを使う。",
            "Use KoeType for this note.",
            "KoeTypeでAPI fallbackを確認する。",
        ] {
            let result = route(&Mode::Raw, &AppSettings::default(), &[], None, text).await;
            assert_eq!(result.text, text);
            assert_eq!(result.polish_state, PolishState::NotRequested);
        }
    }

    #[test]
    fn applied_state_distinguishes_changed_and_unchanged_output() {
        assert_eq!(
            applied_state("同じ本文", "同じ本文"),
            PolishState::AppliedUnchanged
        );
        assert_eq!(
            applied_state("第一段落。第二段落。", "第一段落。\n\n第二段落。"),
            PolishState::AppliedChanged
        );
    }
}
