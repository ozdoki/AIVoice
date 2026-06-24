// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aivoice::{commands, hotkey, local_data, settings, state::AppState, tray};
use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppState::default())
        .setup(|app| {
            // 永続ストアから設定を読み込んで AppState に反映
            let loaded_settings = settings::load(&app.handle()).unwrap_or_default();
            let loaded_dictionary = local_data::load_dictionary(&app.handle()).unwrap_or_default();
            {
                let state = app.state::<AppState>();
                *state.mode.blocking_lock() = loaded_settings.mode.clone();
                *state.settings.blocking_lock() = loaded_settings.clone();
                *state.dictionary_words.blocking_lock() = loaded_dictionary;
            }
            // グローバルホットキーを登録
            hotkey::register_hotkeys(
                &app.handle(),
                hotkey::HotkeySet::new(
                    loaded_settings.push_to_talk_hotkey,
                    loaded_settings.hands_free_hotkey,
                    loaded_settings.toggle_mode_hotkey,
                ),
            )?;
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
            commands::push_to_talk_down,
            commands::push_to_talk_up,
            commands::toggle_hands_free_recording,
            commands::get_settings,
            commands::save_settings,
            commands::save_api_key,
            commands::delete_api_key,
            commands::import_api_key_from_env_file,
            commands::test_api_connection,
            commands::list_models,
            commands::list_audio_devices,
            commands::copy_text,
            commands::inject_text,
            commands::get_history,
            commands::delete_history_item,
            commands::clear_history,
            commands::get_dictionary,
            commands::add_dictionary_word,
            commands::remove_dictionary_word,
            commands::get_usage_summary,
            commands::get_focused_app_context,
            commands::dictionary_limit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
