use tauri::{
    menu::{MenuBuilder, MenuEvent},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

/// トレイアイコンのツールチップを現在の録音状態とモードで更新する。
pub fn update_status(app: &AppHandle, recording_state: &str, mode: &str) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_tooltip(Some(&format!("AIVoice  {mode}  {recording_state}")));
    }
}

const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";

/// システムトレイアイコンを作成し登録する。
pub fn create(app: &AppHandle) -> anyhow::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(MENU_SHOW, "AIVoice を表示")
        .separator()
        .text(MENU_QUIT, "終了")
        .build()?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("AIVoice")
        .menu(&menu)
        .show_menu_on_left_click(false);

    // tauri.conf.json の bundle.icon から読み込まれたウィンドウアイコンをトレイにも使用
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;

    Ok(())
}

/// トレイメニューのクリックを処理する。
pub fn handle_menu(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        MENU_SHOW => {
            let _ = show_main_window(app);
        }
        MENU_QUIT => app.exit(0),
        _ => {}
    }
}

/// トレイアイコン本体のクリックを処理する（左クリックで表示/非表示トグル）。
pub fn handle_tray_event(app: &AppHandle, event: TrayIconEvent) {
    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
        let _ = toggle_main_window(app);
    }
}

fn toggle_main_window(app: &AppHandle) -> tauri::Result<()> {
    let window = app.get_webview_window("main").expect("main window not found");
    if window.is_visible()? {
        window.hide()?;
    } else {
        show_main_window(app)?;
    }
    Ok(())
}

fn show_main_window(app: &AppHandle) -> tauri::Result<()> {
    let window = app.get_webview_window("main").expect("main window not found");
    window.show()?;
    window.set_focus()?;
    Ok(())
}
