// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aivoice::{commands, hotkey, settings, state::AppState};
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::default())
        .setup(|app| {
            // 永続ストアから設定を読み込んで AppState に反映
            if let Ok(s) = settings::load(&app.handle()) {
                let state = app.state::<AppState>();
                *state.settings.blocking_lock() = s;
            }
            // グローバルホットキーを登録
            hotkey::register_hotkeys(&app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_mode,
            commands::set_mode,
            commands::get_recording_state,
            commands::start_recording_session,
            commands::stop_recording_session,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
