use crate::{polish, settings::AppSettings, state::Mode};

/// モードに応じてテキストを加工する。
/// Raw: そのまま返す。Polish: LLM による文章整形。
pub async fn route(mode: &Mode, settings: &AppSettings, text: &str) -> String {
    match mode {
        Mode::Raw => text.to_string(),
        Mode::Polish => match polish::polish_text(settings, text).await {
            Ok(polished) if !polished.trim().is_empty() => polished,
            Ok(_) => text.to_string(),
            Err(e) => {
                tracing::warn!("polish_text failed, falling back to raw: {e}");
                text.to_string()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn raw_mode_returns_text_unchanged() {
        let result = route(&Mode::Raw, &AppSettings::default(), "テスト入力").await;
        assert_eq!(result, "テスト入力");
    }

    #[tokio::test]
    async fn raw_mode_empty_string() {
        let result = route(&Mode::Raw, &AppSettings::default(), "").await;
        assert_eq!(result, "");
    }

    /// Polish モードで api_key が空の場合、polish_text が失敗して raw にフォールバックする。
    #[tokio::test]
    async fn polish_mode_falls_back_to_raw_on_api_error() {
        let settings = AppSettings::default(); // api_key 空 → API 呼び出し失敗
        let result = route(&Mode::Polish, &settings, "元の文").await;
        assert_eq!(result, "元の文");
    }
}
