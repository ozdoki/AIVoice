use tauri::{AppHandle, Emitter};

/// グローバルホットキーを登録する。
///
/// Win32 RegisterHotKey API を使用し、Ctrl+Shift+F4/F5 をグローバルホットキーとして登録する。
/// - Ctrl+Shift+F4 押下: `hotkey://start` を emit（録音開始）
/// - Ctrl+Shift+F4 離す: `hotkey://stop`  を emit（録音停止・テキスト注入）
/// - Ctrl+Shift+F5 押下: `hotkey://toggle-mode` を emit（Raw / Polish 切替）
pub fn register_hotkeys(app: &AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        install_hotkeys(app)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        tracing::warn!("global hotkeys not supported on this platform");
    }
    Ok(())
}

// ---- Windows 実装 -------------------------------------------------------

#[cfg(target_os = "windows")]
use std::sync::{atomic::{AtomicBool, Ordering}, OnceLock};

/// フック→ディスパッチスレッド間チャネルの送信端
#[cfg(target_os = "windows")]
static KEY_SENDER: OnceLock<std::sync::mpsc::SyncSender<u8>> = OnceLock::new();

/// F4 キーアップ ポーリング多重起動防止
#[cfg(target_os = "windows")]
static F4_POLLING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
const EV_F4_DOWN: u8 = 0;
#[cfg(target_os = "windows")]
const EV_F4_UP: u8 = 1;
#[cfg(target_os = "windows")]
const EV_F5_DOWN: u8 = 2;

#[cfg(target_os = "windows")]
const HOTKEY_F4: i32 = 1;
#[cfg(target_os = "windows")]
const HOTKEY_F5: i32 = 2;

#[cfg(target_os = "windows")]
fn install_hotkeys(app: &AppHandle) -> anyhow::Result<()> {
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey,
            HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
        },
        WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY},
    };

    let (tx, rx) = std::sync::mpsc::sync_channel::<u8>(32);
    KEY_SENDER
        .set(tx.clone())
        .map_err(|_| anyhow::anyhow!("KEY_SENDER already initialized"))?;

    // イベントディスパッチスレッド: チャネルから受け取り Tauri emit を呼ぶ
    let app_handle = app.clone();
    std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            match ev {
                EV_F4_DOWN => {
                    let _ = app_handle.emit("hotkey://start", ());
                }
                EV_F4_UP => {
                    let _ = app_handle.emit("hotkey://stop", ());
                }
                EV_F5_DOWN => {
                    let _ = app_handle.emit("hotkey://toggle-mode", ());
                }
                _ => {}
            }
        }
    });

    // ホットキースレッド: RegisterHotKey でグローバルホットキーを登録しメッセージループを回す
    std::thread::spawn(move || unsafe {
        let modifiers: HOT_KEY_MODIFIERS = MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;

        // Ctrl+Shift+F4
        if let Err(e) = RegisterHotKey(None, HOTKEY_F4, modifiers, 0x73 /* VK_F4 */) {
            tracing::error!("RegisterHotKey(Ctrl+Shift+F4) failed: {e}");
            return;
        }
        // Ctrl+Shift+F5
        if let Err(e) = RegisterHotKey(None, HOTKEY_F5, modifiers, 0x74 /* VK_F5 */) {
            tracing::error!("RegisterHotKey(Ctrl+Shift+F5) failed: {e}");
            return;
        }

        tracing::info!("Global hotkeys registered: Ctrl+Shift+F4, Ctrl+Shift+F5");

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                let id = msg.wParam.0 as i32;
                match id {
                    HOTKEY_F4 => {
                        if let Some(sender) = KEY_SENDER.get() {
                            let _ = sender.try_send(EV_F4_DOWN);
                        }
                        // F4 リリースをポーリングで検出
                        if !F4_POLLING.swap(true, Ordering::AcqRel) {
                            let tx2 = tx.clone();
                            std::thread::spawn(move || {
                                poll_f4_release(&tx2);
                                F4_POLLING.store(false, Ordering::Release);
                            });
                        }
                    }
                    HOTKEY_F5 => {
                        if let Some(sender) = KEY_SENDER.get() {
                            let _ = sender.try_send(EV_F5_DOWN);
                        }
                    }
                    _ => {}
                }
            }
        }

        let _ = UnregisterHotKey(None, HOTKEY_F4);
        let _ = UnregisterHotKey(None, HOTKEY_F5);
    });

    Ok(())
}

/// F4 キーが物理的に離されるまでポーリングし、離されたら EV_F4_UP を送信する。
#[cfg(target_os = "windows")]
fn poll_f4_release(tx: &std::sync::mpsc::SyncSender<u8>) {
    use std::time::{Duration, Instant};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    const VK_F4: i32 = 0x73;
    let deadline = Instant::now() + Duration::from_secs(30);

    while Instant::now() < deadline {
        let is_down = unsafe { GetAsyncKeyState(VK_F4) } < 0;
        if !is_down {
            std::thread::sleep(Duration::from_millis(30));
            let _ = tx.try_send(EV_F4_UP);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // タイムアウト: 安全のため停止イベントを送る
    tracing::warn!("F4 release polling timed out (30s)");
    let _ = tx.try_send(EV_F4_UP);
}
