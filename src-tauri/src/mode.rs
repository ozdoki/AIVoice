use crate::state::Mode;

/// モードに応じてテキストを加工する。
/// Raw: そのまま返す。Polish: （将来的に整形処理を行う場所。今は Raw と同じ）
pub fn route(mode: &Mode, text: &str) -> String {
    match mode {
        Mode::Raw => text.to_string(),
        Mode::Polish => text.to_string(), // 将来 PolishProcessor に委譲する
    }
}
