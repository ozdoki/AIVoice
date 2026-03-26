/// F4 キーの物理的な解放を待ってからテキストを注入する。
///
/// F4 キーアップイベントと SendInput の競合を防ぐためのバリア。
/// タイムアウト（250ms）を超えた場合はそのまま注入を続行する。
pub fn inject_text_after_f4(text: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        const VK_F4: i32 = 0x73;
        wait_until_key_up(VK_F4, 250);
    }
    inject_text(text)
}

/// テキストをフォーカス中のアプリのカーソル位置に注入する。
///
/// クリップボード + Ctrl+V 経由で注入する（日本語 IME と干渉しない唯一の安全な経路）。
/// KEYEVENTF_UNICODE は日本語 IME の composition モードで誤動作するため使用しない。
pub fn inject_text(text: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        clipboard_paste(text)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
        anyhow::bail!("inject_text is only supported on Windows");
    }
    Ok(())
}

/// 指定した仮想キーが解放されるまで待つ。timeout_ms を超えたら打ち切る。
#[cfg(target_os = "windows")]
fn wait_until_key_up(vk: i32, timeout_ms: u64) {
    use std::time::{Duration, Instant};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        // GetAsyncKeyState の最上位ビットが 1 → キー押下中
        let is_down = unsafe { GetAsyncKeyState(vk) } < 0;
        if !is_down {
            // キーが離れた後、入力キューが安定するまで少し待つ
            std::thread::sleep(Duration::from_millis(30));
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}


#[cfg(target_os = "windows")]
fn clipboard_paste(text: &str) -> anyhow::Result<()> {
    use arboard::Clipboard;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };

    let mut clipboard = Clipboard::new()?;
    // 現在のクリップボード内容を退避
    let previous_text = clipboard.get_text().ok();

    clipboard.set_text(text)?;

    // Ctrl+V を送信
    let ctrl_v: [INPUT; 4] = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_V,
                    wScan: 0,
                    dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_V,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];

    unsafe { SendInput(&ctrl_v, std::mem::size_of::<INPUT>() as i32) };

    // ペーストが完了するのを待ってからクリップボードを復元
    std::thread::sleep(std::time::Duration::from_millis(150));
    if let Some(prev) = previous_text {
        let _ = clipboard.set_text(&prev);
    }

    Ok(())
}
