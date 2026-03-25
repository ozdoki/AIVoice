use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// グローバルホットキーを登録する。
///
/// - F4 押下: `hotkey://start` を emit（録音開始）
/// - F4 離す: `hotkey://stop`  を emit（録音停止・テキスト注入）
/// - F5 押下: `hotkey://toggle-mode` を emit（Raw / Polish 切替）
///
/// 登録失敗は panic せずログに出力して続行する。
pub fn register_hotkeys(app: &AppHandle) -> anyhow::Result<()> {
    app.global_shortcut()
        .on_shortcut("F4", |app, _shortcut, event| {
            match event.state {
                ShortcutState::Pressed => {
                    let _ = app.emit("hotkey://start", ());
                }
                ShortcutState::Released => {
                    let _ = app.emit("hotkey://stop", ());
                }
            }
        })
        .map_err(|e| {
            tracing::warn!("F4 hotkey registration failed: {e}");
            e
        })?;

    app.global_shortcut()
        .on_shortcut("F5", |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = app.emit("hotkey://toggle-mode", ());
            }
        })
        .map_err(|e| {
            tracing::warn!("F5 hotkey registration failed: {e}");
            e
        })?;

    Ok(())
}
