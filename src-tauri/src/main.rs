// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aivoice::{commands, context, hotkey, local_data, settings, state::AppState, tray};
use tauri::{LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

fn startup_debug(message: &str) {
    let path = std::env::temp_dir().join("aivoice-startup-debug.log");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "{message}")
        });
}

fn main() {
    startup_debug("main: start");
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppState::default())
        .setup(|app| {
            startup_debug("setup: start");
            if app.get_webview_window("main").is_none() {
                startup_debug("setup: main missing, building");
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("KoeType")
                    .inner_size(400.0, 600.0)
                    .resizable(true)
                    .visible(true)
                    .build()?;
                startup_debug("setup: main built");
            } else {
                startup_debug("setup: main exists");
            }
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
                    loaded_settings.hands_free_raw_hotkey,
                    loaded_settings.hands_free_polish_hotkey,
                ),
            )?;
            // システムトレイを作成
            tray::create(&app.handle())?;
            if let Some(window) = app.get_webview_window("main") {
                startup_debug("setup: showing main");
                let _ = window.set_position(LogicalPosition::new(120.0, 120.0));
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            } else {
                startup_debug("setup: main not found after setup");
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if let Some(window) = handle.get_webview_window("main") {
                    startup_debug("delayed: showing main");
                    let _ = window.set_position(LogicalPosition::new(120.0, 120.0));
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                } else {
                    startup_debug("delayed: main not found");
                }
            });
            let focus_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    if let Some(target) = context::current_external_focused_window() {
                        let state = focus_handle.state::<AppState>();
                        *state.last_target_window.lock().await = Some(target);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            });
            startup_debug("setup: done");
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
            commands::resize_and_position_floating_bar,
            commands::start_recording_session,
            commands::stop_recording_session,
            commands::start_onboarding_test_recording,
            commands::stop_onboarding_test_recording,
            commands::push_to_talk_down,
            commands::push_to_talk_up,
            commands::toggle_hands_free_recording,
            commands::toggle_hands_free_recording_for_mode,
            commands::set_active_polish_preset,
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
            commands::toggle_history_pin,
            commands::rerun_history_polish,
            commands::clear_history,
            commands::get_dictionary,
            commands::get_dictionary_suggestions,
            commands::add_dictionary_word,
            commands::remove_dictionary_word,
            commands::get_snippets,
            commands::add_snippet,
            commands::remove_snippet,
            commands::get_usage_summary,
            commands::get_recovery_sessions,
            commands::retry_recovery_session,
            commands::inject_recovery_session,
            commands::save_recovery_session_to_history,
            commands::delete_recovery_session,
            commands::get_focused_app_context,
            commands::dictionary_limit,
            commands::snippet_limit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
