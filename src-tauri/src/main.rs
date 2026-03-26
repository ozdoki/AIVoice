// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aivoice::{commands, hotkey, settings, state::AppState, tray};
use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::default().build())
.manage(AppState::default())
        .setup(|app| {
            // 永続ストアから設定を読み込んで AppState に反映
            if let Ok(s) = settings::load(&app.handle()) {
                let state = app.state::<AppState>();
                *state.mode.blocking_lock() = s.mode.clone();
                *state.settings.blocking_lock() = s;
            }
            // グローバルホットキーを登録
            hotkey::register_hotkeys(&app.handle())?;
            // システムトレイを作成
            tray::create(&app.handle())?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            tray::handle_menu(app, event);
        })
        .on_tray_icon_event(|app, event| {
            tray::handle_tray_event(app, event);
        })
        .on_window_event(|window, event| {
            // X ボタンでは終了せず hide してトレイに残す
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_mode,
            commands::set_mode,
            commands::get_recording_state,
            commands::start_recording_session,
            commands::stop_recording_session,
            commands::get_settings,
            commands::save_settings,
            commands::list_audio_devices,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
