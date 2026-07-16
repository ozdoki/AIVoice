/// テキストをフォーカス中のアプリのカーソル位置に注入する。
///
/// 互換性の高いクリップボード貼付を使い、変換結果はクリップボードに残す。
pub fn inject_text(text: &str) -> anyhow::Result<InjectionSuccess> {
    #[cfg(target_os = "windows")]
    {
        let success = clipboard_paste(text, None)?;
        if let Some(warning) = &success.warning {
            tracing::warn!("{warning}");
        }
        return Ok(success);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
        anyhow::bail!("inject_text is only supported on Windows");
    }
}

pub fn inject_text_to_window(
    text: &str,
    target: &crate::context::FocusedWindowTarget,
) -> anyhow::Result<InjectionSuccess> {
    if let Err(error) = crate::context::focus_window(target) {
        return Err(error_with_clipboard_backup(text, error));
    }
    #[cfg(target_os = "windows")]
    let success = clipboard_paste(text, Some(target))?;
    if let Some(warning) = &success.warning {
        tracing::warn!("{warning}");
    }
    return Ok(success);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (text, target);
        anyhow::bail!("inject_text_to_window is only supported on Windows")
    }
}

#[derive(Debug)]
pub struct ReplacementInjectionFailure {
    pub partial: bool,
    pub message: String,
}

pub struct ReplacementInjectionSuccess {
    pub warning: Option<String>,
}

/// 選択範囲置換専用。対象一致を確認してclipboard貼付し、結果を残す。
/// Ctrl+Vの部分送信時は二重挿入防止のため再試行せず、提案全文を回収可能にする。
pub fn inject_clipboard_replacement(
    text: &str,
    expected_target: &crate::context::FocusedWindowTarget,
) -> Result<ReplacementInjectionSuccess, ReplacementInjectionFailure> {
    #[cfg(target_os = "windows")]
    {
        return clipboard_paste(text, Some(expected_target))
            .map(|success| ReplacementInjectionSuccess {
                warning: success.warning,
            })
            .map_err(|error| ReplacementInjectionFailure {
                partial: error
                    .downcast_ref::<ClipboardPasteFailure>()
                    .is_some_and(|failure| failure.partial),
                message: error.to_string(),
            });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (text, expected_target);
        Err(ReplacementInjectionFailure {
            partial: false,
            message: "選択範囲の直接置換はWindowsでのみ利用できます。".to_string(),
        })
    }
}

#[cfg(target_os = "windows")]
fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text)?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn error_with_clipboard_backup(text: &str, error: anyhow::Error) -> anyhow::Error {
    match copy_to_clipboard(text) {
        Ok(()) => anyhow::anyhow!(
            "自動挿入に失敗しました: {error}。変換結果はクリップボードに保存しました。"
        ),
        Err(backup_error) => anyhow::anyhow!(
            "自動挿入に失敗しました: {error}。さらに、変換結果をクリップボードへ退避できませんでした: {backup_error}"
        ),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn error_with_clipboard_backup(_text: &str, error: anyhow::Error) -> anyhow::Error {
    error
}

#[cfg(target_os = "windows")]
fn keyboard_input(
    virtual_key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
    scan_code: u16,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_KEYBOARD};

    // INPUT_0 is sized for its largest union member (MOUSEINPUT). Building it from the
    // smaller KEYBDINPUT member leaves the remaining bytes uninitialized. Windows examples
    // zero the complete INPUT before selecting the keyboard member; do the same so the full
    // cbSize-sized value passed to SendInput is deterministic.
    let mut input = INPUT::default();
    input.r#type = INPUT_KEYBOARD;
    // Write fields in place. Assigning a KEYBDINPUT value can make its internal alignment
    // padding indeterminate again even when the destination union was initially zeroed.
    unsafe {
        let keyboard = &mut input.Anonymous.ki;
        keyboard.wVk = virtual_key;
        keyboard.wScan = scan_code;
        keyboard.dwFlags = flags;
        keyboard.time = 0;
        keyboard.dwExtraInfo = 0;
    }
    input
}

#[cfg(target_os = "windows")]
fn foreground_matches(expected: &crate::context::FocusedWindowTarget) -> bool {
    crate::context::focused_window_target().is_some_and(|current| {
        current.hwnd == expected.hwnd && current.process_id == expected.process_id
    })
}

#[cfg(target_os = "windows")]
fn send_ctrl_v() -> CtrlVSendResult {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };
    let inputs = [
        keyboard_input(VK_CONTROL, 0, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_V, 0, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_V, 0, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
    match sent {
        0 => CtrlVSendResult::Zero,
        value if value == inputs.len() => CtrlVSendResult::Full,
        _ => CtrlVSendResult::Partial,
    }
}

#[cfg(target_os = "windows")]
fn release_ctrl_v_keys_best_effort() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };
    let releases = [
        keyboard_input(VK_V, 0, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
    ];
    let _ = unsafe { SendInput(&releases, std::mem::size_of::<INPUT>() as i32) };
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct ClipboardPasteFailure {
    partial: bool,
    message: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CtrlVSendResult {
    Zero,
    Partial,
    Full,
}

#[derive(Debug)]
pub struct InjectionSuccess {
    pub warning: Option<String>,
}

trait ClipboardPasteBackend {
    type Prepared;

    fn prepare(&mut self, text: &str) -> Result<Self::Prepared, ()>;
    fn target_matches(&mut self) -> bool;
    fn preflight(
        &mut self,
        prepared: &mut Self::Prepared,
        expected_text: &str,
    ) -> crate::selection::ClipboardPreflight;
    fn send(&mut self) -> CtrlVSendResult;
    fn release_keys_best_effort(&mut self);
}

fn run_clipboard_paste<B: ClipboardPasteBackend>(
    backend: &mut B,
    text: &str,
) -> Result<InjectionSuccess, ClipboardPasteFailure> {
    let mut prepared = backend.prepare(text).map_err(|_| ClipboardPasteFailure {
        partial: false,
        message: "現在のクリップボードを安全に準備できないため、入力を中止しました。",
    })?;
    if !backend.target_matches() {
        return Err(ClipboardPasteFailure {
            partial: false,
            message: "入力先のフォーカスが変わったため、貼り付け前に中止しました。変換結果はクリップボードに残しています。",
        });
    }
    match backend.preflight(&mut prepared, text) {
        crate::selection::ClipboardPreflight::Current
        | crate::selection::ClipboardPreflight::Equivalent => {}
        crate::selection::ClipboardPreflight::Different => {
            return Err(ClipboardPasteFailure {
                partial: false,
                message: "貼り付け前にクリップボード本文が別の内容へ置き換わったため、入力を中止しました。",
            });
        }
        crate::selection::ClipboardPreflight::Unavailable => {
            return Err(ClipboardPasteFailure {
                partial: false,
                message: "貼り付け前のクリップボード所有状態を確認できないため、入力を中止しました。変換結果または外部更新がクリップボードに残っています。",
            });
        }
    }
    // Clipboard preflight can yield to another process. Re-check the foreground
    // target immediately before SendInput so a focus change during preflight
    // aborts without sending Ctrl+V to an unintended window. A race after this
    // check remains possible at the OS input boundary and is reported below.
    if !backend.target_matches() {
        return Err(ClipboardPasteFailure {
            partial: false,
            message: "貼り付け直前に入力先のフォーカスが変わったため、入力を中止しました。変換結果はクリップボードに残しています。",
        });
    }
    match backend.send() {
        CtrlVSendResult::Zero => {
            Err(ClipboardPasteFailure {
                partial: false,
                message: "貼り付け操作を送信できませんでした。変換結果はクリップボードに残しています。自動再試行は行いません。",
            })
        }
        CtrlVSendResult::Partial => {
            backend.release_keys_best_effort();
            Err(ClipboardPasteFailure {
                partial: true,
                message: "貼り付け操作が途中で失敗しました。二重入力を避けるため自動再試行は行わず、変換結果をクリップボードに残しました。",
            })
        }
        CtrlVSendResult::Full => {
            let focus_changed = !backend.target_matches();
            let warning = focus_changed.then_some("貼り付け操作は送信済みですが、直後に入力先の変化を検出しました。二重入力防止のため成功扱いとし、自動再試行は行いません。");
            Ok(InjectionSuccess {
                warning: warning.map(str::to_string),
            })
        }
    }
}

#[cfg(target_os = "windows")]
impl std::fmt::Display for ClipboardPasteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

#[cfg(target_os = "windows")]
impl std::error::Error for ClipboardPasteFailure {}

#[cfg(target_os = "windows")]
struct SystemClipboardPasteBackend<'a> {
    expected_target: Option<&'a crate::context::FocusedWindowTarget>,
}

#[cfg(target_os = "windows")]
impl ClipboardPasteBackend for SystemClipboardPasteBackend<'_> {
    type Prepared = crate::selection::ClipboardPasteTransaction;

    fn prepare(&mut self, text: &str) -> Result<Self::Prepared, ()> {
        crate::selection::ClipboardPasteTransaction::begin(text).map_err(|_| ())
    }

    fn target_matches(&mut self) -> bool {
        self.expected_target
            .is_none_or(|target| foreground_matches(target))
    }

    fn preflight(
        &mut self,
        prepared: &mut Self::Prepared,
        expected_text: &str,
    ) -> crate::selection::ClipboardPreflight {
        prepared.preflight(expected_text)
    }

    fn send(&mut self) -> CtrlVSendResult {
        send_ctrl_v()
    }

    fn release_keys_best_effort(&mut self) {
        release_ctrl_v_keys_best_effort();
    }
}

#[cfg(target_os = "windows")]
fn clipboard_paste(
    text: &str,
    expected_target: Option<&crate::context::FocusedWindowTarget>,
) -> anyhow::Result<InjectionSuccess> {
    let mut backend = SystemClipboardPasteBackend { expected_target };
    let success = run_clipboard_paste(&mut backend, text)?;
    Ok(success)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_UNICODE;

    #[test]
    fn keyboard_input_zeroes_the_unused_union_tail() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT_0, KEYBDINPUT, VIRTUAL_KEY};

        let input = keyboard_input(VIRTUAL_KEY(0), 'A' as u16, KEYEVENTF_UNICODE);
        let tail_len = std::mem::size_of::<INPUT_0>() - std::mem::size_of::<KEYBDINPUT>();
        let tail = unsafe {
            std::slice::from_raw_parts(
                (&input.Anonymous as *const INPUT_0 as *const u8)
                    .add(std::mem::size_of::<KEYBDINPUT>()),
                tail_len,
            )
        };
        assert!(tail.iter().all(|byte| *byte == 0));
    }

    struct FakeBackend {
        prepare_ok: bool,
        focus: Vec<bool>,
        focus_index: usize,
        send: CtrlVSendResult,
        preflight: crate::selection::ClipboardPreflight,
        expected_text: Option<String>,
        send_calls: usize,
        release_calls: usize,
    }

    impl FakeBackend {
        fn valid(send: CtrlVSendResult) -> Self {
            Self {
                prepare_ok: true,
                focus: vec![true, true],
                focus_index: 0,
                send,
                preflight: crate::selection::ClipboardPreflight::Current,
                expected_text: None,
                send_calls: 0,
                release_calls: 0,
            }
        }
    }

    impl ClipboardPasteBackend for FakeBackend {
        type Prepared = ();

        fn prepare(&mut self, _text: &str) -> Result<(), ()> {
            self.prepare_ok.then_some(()).ok_or(())
        }

        fn target_matches(&mut self) -> bool {
            let value = self.focus.get(self.focus_index).copied().unwrap_or(true);
            self.focus_index += 1;
            value
        }

        fn preflight(
            &mut self,
            _prepared: &mut (),
            expected_text: &str,
        ) -> crate::selection::ClipboardPreflight {
            self.expected_text = Some(expected_text.to_string());
            self.preflight
        }

        fn send(&mut self) -> CtrlVSendResult {
            self.send_calls += 1;
            self.send
        }

        fn release_keys_best_effort(&mut self) {
            self.release_calls += 1;
        }
    }

    #[test]
    fn prepare_failure_stops_before_send_without_snapshot_policy() {
        let mut backend = FakeBackend::valid(CtrlVSendResult::Full);
        backend.prepare_ok = false;
        assert!(run_clipboard_paste(&mut backend, "secret").is_err());
        assert_eq!(backend.send_calls, 0);
    }

    #[test]
    fn preflight_requires_exact_unicode_text_before_send() {
        let text = "一行目\r\n😀 二行目";
        let mut backend = FakeBackend::valid(CtrlVSendResult::Full);
        run_clipboard_paste(&mut backend, text).unwrap();
        assert_eq!(backend.expected_text.as_deref(), Some(text));
        assert_eq!(backend.send_calls, 1);

        let mut changed = FakeBackend::valid(CtrlVSendResult::Full);
        changed.preflight = crate::selection::ClipboardPreflight::Different;
        let failure = run_clipboard_paste(&mut changed, "secret").unwrap_err();
        assert!(failure.message.contains("本文が別の内容"));
        assert!(!failure.message.contains("secret"));
        assert_eq!(changed.send_calls, 0);
    }

    #[test]
    fn focus_change_during_preflight_stops_before_send() {
        let mut backend = FakeBackend::valid(CtrlVSendResult::Full);
        backend.focus = vec![true, false];
        let failure = run_clipboard_paste(&mut backend, "secret").unwrap_err();
        assert!(failure.message.contains("貼り付け直前"));
        assert_eq!(backend.send_calls, 0);
    }

    #[test]
    fn zero_and_partial_send_leave_result_without_restore() {
        let mut zero = FakeBackend::valid(CtrlVSendResult::Zero);
        let zero_failure = run_clipboard_paste(&mut zero, "secret").unwrap_err();
        assert!(!zero_failure.partial);
        assert!(zero_failure
            .message
            .contains("クリップボードに残しています"));

        let mut partial = FakeBackend::valid(CtrlVSendResult::Partial);
        let partial_failure = run_clipboard_paste(&mut partial, "secret").unwrap_err();
        assert!(partial_failure.partial);
        assert_eq!(partial.release_calls, 1);
        assert!(!partial_failure.message.contains("secret"));
    }

    #[test]
    fn full_send_with_focus_change_is_uncertain_success_without_cleanup() {
        let mut backend = FakeBackend::valid(CtrlVSendResult::Full);
        backend.focus = vec![true, true, false];
        assert!(run_clipboard_paste(&mut backend, "secret")
            .unwrap()
            .warning
            .is_some());
    }
}
