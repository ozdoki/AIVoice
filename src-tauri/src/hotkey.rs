use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// グローバルホットキーを登録する。
///
/// Win32 WH_KEYBOARD_LL フックを使用し、F4/F5 キーを他アプリに漏らさず抑制する。
/// - F4 押下: `hotkey://start` を emit（録音開始）
/// - F4 離す: `hotkey://stop`  を emit（録音停止・テキスト注入）
/// - F5 押下: `hotkey://toggle-mode` を emit（Raw / Polish 切替）
pub fn register_hotkeys(app: &AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        install_ll_hook(app)?;
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
use std::sync::OnceLock;

/// フック→ディスパッチスレッド間チャネルの送信端
#[cfg(target_os = "windows")]
static KEY_SENDER: OnceLock<std::sync::mpsc::SyncSender<u8>> = OnceLock::new();

/// F4 長押し時のキーリピートによる多重 emit を防ぐフラグ
#[cfg(target_os = "windows")]
static F4_HELD: AtomicBool = AtomicBool::new(false);
/// F5 キーリピート防止フラグ
#[cfg(target_os = "windows")]
static F5_HELD: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
const EV_F4_DOWN: u8 = 0;
#[cfg(target_os = "windows")]
const EV_F4_UP: u8 = 1;
#[cfg(target_os = "windows")]
const EV_F5_DOWN: u8 = 2;

#[cfg(target_os = "windows")]
const VK_F4: u32 = 0x73;
#[cfg(target_os = "windows")]
const VK_F5: u32 = 0x74;

/// 低レベルキーボードフック コールバック
///
/// F4/F5 を検出したら KEY_SENDER に送信し LRESULT(1) を返してキーを消費する。
/// それ以外のキーは CallNextHookEx でパスする。
#[cfg(target_os = "windows")]
unsafe extern "system" fn ll_keyboard_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::{
        Foundation::LRESULT,
        UI::WindowsAndMessaging::{
            CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT,
            WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    };

    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

        match kb.vkCode {
            VK_F4 => {
                if let Some(tx) = KEY_SENDER.get() {
                    if is_down {
                        // キーリピートを無視: 最初の押下のみ emit
                        if !F4_HELD.swap(true, Ordering::AcqRel) {
                            let _ = tx.try_send(EV_F4_DOWN);
                        }
                    } else if is_up {
                        F4_HELD.store(false, Ordering::Release);
                        let _ = tx.try_send(EV_F4_UP);
                    }
                }
                return LRESULT(1); // キーを消費（他アプリに渡さない）
            }
            VK_F5 if is_down => {
                // キーリピートを無視: 最初の押下のみ emit
                if !F5_HELD.swap(true, Ordering::AcqRel) {
                    if let Some(tx) = KEY_SENDER.get() {
                        let _ = tx.try_send(EV_F5_DOWN);
                    }
                }
                return LRESULT(1);
            }
            VK_F5 if is_up => {
                F5_HELD.store(false, Ordering::Release);
                return LRESULT(1);
            }
            _ => {}
        }
    }

    CallNextHookEx(HHOOK(std::ptr::null_mut()), code, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn install_ll_hook(app: &AppHandle) -> anyhow::Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, WH_KEYBOARD_LL,
    };

    let (tx, rx) = std::sync::mpsc::sync_channel::<u8>(32);
    KEY_SENDER
        .set(tx)
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

    // フックスレッド: WH_KEYBOARD_LL をインストールしてメッセージループを回す
    std::thread::spawn(|| unsafe {
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("WH_KEYBOARD_LL install failed: {e}");
                return;
            }
        };

        let mut msg = MSG::default();
        // GetMessageW がメッセージループを維持しフックコールバックを動かし続ける
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}

        let _ = UnhookWindowsHookEx(hook);
    });

    Ok(())
}
