use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusedAppContext {
    pub process_name: String,
    pub window_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusedWindowTarget {
    pub hwnd: isize,
    pub process_id: u32,
    pub process_name: String,
    pub window_title: String,
}

#[cfg(target_os = "windows")]
pub fn focused_window_target() -> Option<FocusedWindowTarget> {
    use std::path::Path;

    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            },
        },
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        let title_len = GetWindowTextLengthW(hwnd);
        let mut title_buf = vec![0_u16; title_len.saturating_add(1) as usize];
        let title_read = GetWindowTextW(hwnd, &mut title_buf);
        let window_title = String::from_utf16_lossy(&title_buf[..title_read as usize]);

        let mut pid = 0_u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return Some(FocusedWindowTarget {
                hwnd: hwnd.0 as isize,
                process_id: pid,
                process_name: String::new(),
                window_title,
            });
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path_buf = vec![0_u16; 1024];
        let mut size = path_buf.len() as u32;
        let process_name = if QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            PWSTR(path_buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
        {
            let full_path = String::from_utf16_lossy(&path_buf[..size as usize]);
            Path::new(&full_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&full_path)
                .to_string()
        } else {
            String::new()
        };
        let _ = CloseHandle(process);

        Some(FocusedWindowTarget {
            hwnd: hwnd.0 as isize,
            process_id: pid,
            process_name,
            window_title,
        })
    }
}

#[cfg(not(target_os = "windows"))]
pub fn focused_window_target() -> Option<FocusedWindowTarget> {
    None
}

pub fn focused_app_context() -> Option<FocusedAppContext> {
    focused_window_target().map(|target| FocusedAppContext {
        process_name: target.process_name,
        window_title: target.window_title,
    })
}

pub fn current_external_focused_window() -> Option<FocusedWindowTarget> {
    let target = focused_window_target()?;
    if target.process_id == std::process::id() {
        return None;
    }
    Some(target)
}

#[cfg(target_os = "windows")]
pub fn focus_window(target: &FocusedWindowTarget) -> anyhow::Result<()> {
    use std::{ffi::c_void, thread, time::Duration};

    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            IsIconic, IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
        },
    };

    unsafe {
        let hwnd = HWND(target.hwnd as *mut c_void);
        if !IsWindow(hwnd).as_bool() {
            anyhow::bail!("以前の入力先ウィンドウが見つかりません。入力先アプリを一度クリックしてから再注入してください。");
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            anyhow::bail!("入力先アプリへフォーカスを戻せませんでした。入力先アプリを一度クリックしてから再注入してください。");
        }
    }
    thread::sleep(Duration::from_millis(140));
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn focus_window(_target: &FocusedWindowTarget) -> anyhow::Result<()> {
    anyhow::bail!("window focus restore is only supported on Windows")
}

pub fn prompt_fragment(context: Option<&FocusedAppContext>) -> String {
    let Some(context) = context else {
        return String::new();
    };
    let mut lines = Vec::new();
    if !context.process_name.trim().is_empty() {
        lines.push(format!("Input target app: {}", context.process_name.trim()));
    }
    if !context.window_title.trim().is_empty() {
        lines.push(format!(
            "Input target window title: {}",
            context.window_title.trim()
        ));
    }
    lines.join("\n")
}

fn context_text(context: &FocusedAppContext) -> String {
    format!(
        "{}\n{}",
        context.process_name.trim().to_ascii_lowercase(),
        context.window_title.trim().to_ascii_lowercase()
    )
}

pub fn suggested_polish_preset(context: Option<&FocusedAppContext>, fallback: &str) -> String {
    let Some(context) = context else {
        return normalized_polish_preset(fallback);
    };
    let text = context_text(context);
    if text.contains("slack")
        || text.contains("teams")
        || text.contains("discord")
        || text.contains("chatwork")
        || text.contains("line")
    {
        "slack".to_string()
    } else if text.contains("outlook")
        || text.contains("thunderbird")
        || text.contains("gmail")
        || text.contains("mail")
    {
        "email".to_string()
    } else if text.contains("code")
        || text.contains("cursor")
        || text.contains("visual studio")
        || text.contains("jetbrains")
        || text.contains("intellij")
        || text.contains("rustrover")
        || text.contains("webstorm")
        || text.contains("pycharm")
        || text.contains("terminal")
        || text.contains("powershell")
    {
        "technical".to_string()
    } else if text.contains("chatgpt")
        || text.contains("claude")
        || text.contains("gemini")
        || text.contains("copilot")
    {
        "prompt".to_string()
    } else {
        normalized_polish_preset(fallback)
    }
}

fn normalized_polish_preset(preset: &str) -> String {
    match preset.trim() {
        "slack" | "email" | "memo" | "prompt" | "technical" => preset.trim().to_string(),
        _ => "memo".to_string(),
    }
}

pub fn app_style_hint(context: Option<&FocusedAppContext>) -> String {
    let Some(context) = context else {
        return String::new();
    };
    let text = context_text(context);
    let hint = if text.contains("slack")
        || text.contains("teams")
        || text.contains("discord")
        || text.contains("chatwork")
        || text.contains("line")
    {
        "Chat app style hint: keep the text concise, direct, and easy to send as a short message. Avoid email-like openings, closings, and signatures."
    } else if text.contains("outlook")
        || text.contains("thunderbird")
        || text.contains("gmail")
        || text.contains("mail")
    {
        "Email app style hint: use a polite email-body tone with clear paragraphs. Do not invent a subject, recipient, sender, or signature."
    } else if text.contains("code")
        || text.contains("cursor")
        || text.contains("visual studio")
        || text.contains("jetbrains")
        || text.contains("intellij")
        || text.contains("rustrover")
        || text.contains("webstorm")
        || text.contains("pycharm")
        || text.contains("terminal")
        || text.contains("powershell")
    {
        "IDE or terminal style hint: preserve technical terms, code identifiers, commands, file paths, branch names, issue numbers, and symbols exactly where possible."
    } else if text.contains("chrome")
        || text.contains("edge")
        || text.contains("firefox")
        || text.contains("browser")
    {
        "Browser style hint: keep the text suitable for a web text field. Prefer concise wording and avoid app-specific formatting unless the transcript asks for it."
    } else {
        ""
    };
    hint.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_fragment_omits_missing_context() {
        assert!(prompt_fragment(None).is_empty());
        let context = FocusedAppContext {
            process_name: "notepad.exe".to_string(),
            window_title: "memo".to_string(),
        };
        let prompt = prompt_fragment(Some(&context));
        assert!(prompt.contains("notepad.exe"));
        assert!(prompt.contains("memo"));
    }

    #[test]
    fn app_style_hint_detects_chat_email_browser_and_ide() {
        let slack = FocusedAppContext {
            process_name: "Slack.exe".to_string(),
            window_title: "general".to_string(),
        };
        assert!(app_style_hint(Some(&slack)).contains("Chat app"));

        let outlook = FocusedAppContext {
            process_name: "OUTLOOK.EXE".to_string(),
            window_title: "Inbox".to_string(),
        };
        assert!(app_style_hint(Some(&outlook)).contains("Email app"));

        let code = FocusedAppContext {
            process_name: "Code.exe".to_string(),
            window_title: "KoeType - Visual Studio Code".to_string(),
        };
        assert!(app_style_hint(Some(&code)).contains("IDE or terminal"));

        let edge = FocusedAppContext {
            process_name: "msedge.exe".to_string(),
            window_title: "ChatGPT".to_string(),
        };
        assert!(app_style_hint(Some(&edge)).contains("Browser style"));
    }

    #[test]
    fn suggested_polish_preset_detects_supported_app_groups() {
        let slack = FocusedAppContext {
            process_name: "Slack.exe".to_string(),
            window_title: "general".to_string(),
        };
        assert_eq!(suggested_polish_preset(Some(&slack), "memo"), "slack");

        let gmail = FocusedAppContext {
            process_name: "chrome.exe".to_string(),
            window_title: "Gmail - Inbox".to_string(),
        };
        assert_eq!(suggested_polish_preset(Some(&gmail), "memo"), "email");

        let code = FocusedAppContext {
            process_name: "Code.exe".to_string(),
            window_title: "KoeType - Visual Studio Code".to_string(),
        };
        assert_eq!(suggested_polish_preset(Some(&code), "memo"), "technical");

        let chatgpt = FocusedAppContext {
            process_name: "msedge.exe".to_string(),
            window_title: "ChatGPT".to_string(),
        };
        assert_eq!(suggested_polish_preset(Some(&chatgpt), "memo"), "prompt");
    }

    #[test]
    fn suggested_polish_preset_uses_normalized_fallback() {
        let notepad = FocusedAppContext {
            process_name: "notepad.exe".to_string(),
            window_title: "draft".to_string(),
        };

        assert_eq!(suggested_polish_preset(Some(&notepad), "email"), "email");
        assert_eq!(suggested_polish_preset(None, "unknown"), "memo");
    }
}
