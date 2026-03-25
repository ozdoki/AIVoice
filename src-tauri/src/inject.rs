/// テキストをフォーカス中のアプリのカーソル位置に注入する。
///
/// 第一経路: SendInput（Win32 API による直接入力）
/// 第二経路: クリップボード + Ctrl+V（フォールバック）
pub fn inject_text(text: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        if send_input(text).is_ok() {
            return Ok(());
        }
        tracing::warn!("SendInput failed, falling back to clipboard");
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
fn send_input(text: &str) -> anyhow::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_UNICODE,
    };

    let mut inputs: Vec<INPUT> = Vec::new();

    for ch in text.encode_utf16() {
        // key down
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                    wScan: ch,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
        // key up
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                    wScan: ch,
                    dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_UNICODE
                        | windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        anyhow::bail!("SendInput: only {}/{} events sent", sent, inputs.len());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn clipboard_paste(text: &str) -> anyhow::Result<()> {
    use arboard::Clipboard;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };

    let mut clipboard = Clipboard::new()?;
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
    Ok(())
}
