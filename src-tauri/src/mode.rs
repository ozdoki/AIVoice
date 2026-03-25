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
