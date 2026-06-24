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

#[cfg(target_os = "windows")]
fn clipboard_paste(text: &str) -> anyhow::Result<()> {
    use arboard::Clipboard;
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };

    let mut clipboard = Clipboard::new()?;
    // 現在のクリップボード内容を退避
    let previous_text = clipboard.get_text().ok();

    // クリップボード変更シーケンス番号を記録（set_text 前）
    let seq_before = unsafe { GetClipboardSequenceNumber() };

    clipboard.set_text(text)?;
    // この時点でシーケンス番号は seq_before + 1 になっているはず

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

    // 待機中に別プロセスがクリップボードを書き換えた場合は復元しない。
    // seq_before + 1 == 自分の set_text のみ。それ以上なら外部変更あり。
    let seq_after = unsafe { GetClipboardSequenceNumber() };
    if seq_after == seq_before.wrapping_add(1) {
        if let Some(prev) = previous_text {
            let _ = clipboard.set_text(&prev);
        }
    } else {
        tracing::debug!(
            "clipboard modified by another process during injection (seq {} -> {}), skipping restore",
            seq_before, seq_after
        );
    }

    Ok(())
}
