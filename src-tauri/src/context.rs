use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusedAppContext {
    pub process_name: String,
    pub window_title: String,
}

#[cfg(target_os = "windows")]
pub fn focused_app_context() -> Option<FocusedAppContext> {
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
                GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
                GetWindowThreadProcessId,
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
            return Some(FocusedAppContext {
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

        Some(FocusedAppContext {
            process_name,
            window_title,
        })
    }
}

#[cfg(not(target_os = "windows"))]
pub fn focused_app_context() -> Option<FocusedAppContext> {
    None
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
        lines.push(format!("Input target window title: {}", context.window_title.trim()));
    }
    lines.join("\n")
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
}
