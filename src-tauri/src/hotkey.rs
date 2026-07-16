use std::collections::HashSet;

use tauri::AppHandle;

use crate::settings::HotkeyBinding;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySet {
    pub push_to_talk: HotkeyBinding,
    pub hands_free_raw: HotkeyBinding,
    pub hands_free_polish: HotkeyBinding,
    pub learn_selected: HotkeyBinding,
    pub voice_edit_selected: HotkeyBinding,
}

impl HotkeySet {
    pub fn new(
        push_to_talk: HotkeyBinding,
        hands_free_raw: HotkeyBinding,
        hands_free_polish: HotkeyBinding,
        learn_selected: HotkeyBinding,
        voice_edit_selected: HotkeyBinding,
    ) -> Self {
        Self {
            push_to_talk,
            hands_free_raw,
            hands_free_polish,
            learn_selected,
            voice_edit_selected,
        }
    }

    pub fn validate_and_normalize(mut self) -> Result<Self, String> {
        self.push_to_talk = normalize_binding(&self.push_to_talk)?;
        self.hands_free_raw = normalize_binding(&self.hands_free_raw)?;
        self.hands_free_polish = normalize_binding(&self.hands_free_polish)?;
        self.learn_selected = normalize_binding(&self.learn_selected)?;
        self.voice_edit_selected = normalize_binding(&self.voice_edit_selected)?;

        let bindings = [
            &self.push_to_talk,
            &self.hands_free_raw,
            &self.hands_free_polish,
            &self.learn_selected,
            &self.voice_edit_selected,
        ];
        let mut seen = HashSet::new();
        for binding in bindings {
            let signature = binding_signature(binding);
            if !seen.insert(signature) {
                return Err("ショートカットキーが重複しています。".to_string());
            }
        }
        Ok(self)
    }
}

fn normalize_binding(binding: &HotkeyBinding) -> Result<HotkeyBinding, String> {
    let key = normalize_key_name(&binding.key)
        .ok_or_else(|| format!("未対応のショートカットキーです: {}", binding.key))?;
    Ok(HotkeyBinding {
        ctrl: binding.ctrl,
        alt: binding.alt,
        shift: binding.shift,
        key,
    })
}

fn binding_signature(binding: &HotkeyBinding) -> String {
    format!(
        "{}:{}:{}:{}",
        binding.ctrl,
        binding.alt,
        binding.shift,
        binding.key.to_ascii_uppercase()
    )
}

fn normalize_key_name(input: &str) -> Option<String> {
    let key = input.trim().to_ascii_uppercase();
    if key.len() == 1 {
        let c = key.chars().next()?;
        if c.is_ascii_alphanumeric() {
            return Some(key);
        }
    }
    if let Some(number) = key.strip_prefix('F').and_then(|s| s.parse::<u8>().ok()) {
        if (1..=24).contains(&number) {
            return Some(format!("F{number}"));
        }
    }
    let canonical = match key.as_str() {
        "SPACE" | "SPACEBAR" => "Space",
        "ENTER" | "RETURN" => "Enter",
        "TAB" => "Tab",
        "ESC" | "ESCAPE" => "Esc",
        "ARROWUP" | "UP" => "ArrowUp",
        "ARROWDOWN" | "DOWN" => "ArrowDown",
        "ARROWLEFT" | "LEFT" => "ArrowLeft",
        "ARROWRIGHT" | "RIGHT" => "ArrowRight",
        "BACKSPACE" => "Backspace",
        "DELETE" | "DEL" => "Delete",
        "INSERT" | "INS" => "Insert",
        "HOME" => "Home",
        "END" => "End",
        "PAGEUP" | "PGUP" => "PageUp",
        "PAGEDOWN" | "PGDN" => "PageDown",
        _ => return None,
    };
    Some(canonical.to_string())
}

fn replace_registered_set<U, R>(
    current: &HotkeySet,
    next: &HotkeySet,
    unregister: &mut U,
    register: &mut R,
) -> Result<(), String>
where
    U: FnMut(),
    R: FnMut(&HotkeySet) -> Result<(), String>,
{
    unregister();
    match register(next) {
        Ok(()) => Ok(()),
        Err(error) => {
            unregister();
            if let Err(restore_error) = register(current) {
                tracing::error!("failed to restore previous hotkeys: {restore_error}");
                return Err(format!(
                    "{error} 以前のショートカットの復元にも失敗しました: {restore_error}"
                ));
            }
            Err(error)
        }
    }
}

pub fn register_hotkeys(app: &AppHandle, bindings: HotkeySet) -> anyhow::Result<()> {
    let bindings = bindings
        .validate_and_normalize()
        .map_err(anyhow::Error::msg)?;
    #[cfg(target_os = "windows")]
    {
        windows_impl::install(app, bindings)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        let _ = bindings;
        tracing::warn!("global hotkeys not supported on this platform");
    }
    Ok(())
}

pub fn reconfigure_hotkeys(bindings: HotkeySet) -> Result<HotkeySet, String> {
    let normalized = bindings.validate_and_normalize()?;
    #[cfg(target_os = "windows")]
    {
        windows_impl::reconfigure(normalized.clone())?;
    }
    Ok(normalized)
}

/// 録音中だけEscapeをグローバルに捕捉する。一時登録であり、検出時は即座に解除される。
pub fn set_cancel_hotkey_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::set_cancel_enabled(enabled)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc::{self, Receiver, SyncSender, TryRecvError},
            OnceLock,
        },
        time::Duration,
    };

    use tauri::{AppHandle, Emitter, Manager};
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT,
            MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
        },
        WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY},
    };

    use super::{HotkeyBinding, HotkeySet};

    const HOTKEY_PUSH_TO_TALK: i32 = 1;
    const HOTKEY_HANDS_FREE_RAW: i32 = 2;
    const HOTKEY_HANDS_FREE_POLISH: i32 = 3;
    const HOTKEY_LEARN_SELECTED: i32 = 4;
    const HOTKEY_VOICE_EDIT_SELECTED: i32 = 5;
    const HOTKEY_CANCEL_RECORDING: i32 = 6;

    static MANAGER: OnceLock<HotkeyManager> = OnceLock::new();
    static PTT_POLLING: AtomicBool = AtomicBool::new(false);

    enum WorkerCommand {
        Reconfigure {
            bindings: HotkeySet,
            reply: SyncSender<Result<(), String>>,
        },
        SetCancelEnabled {
            enabled: bool,
            reply: SyncSender<Result<(), String>>,
        },
    }

    struct HotkeyManager {
        command_tx: SyncSender<WorkerCommand>,
    }

    pub fn install(app: &AppHandle, bindings: HotkeySet) -> anyhow::Result<()> {
        let (command_tx, command_rx) = mpsc::sync_channel::<WorkerCommand>(8);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let app_handle = app.clone();

        std::thread::spawn(move || worker_loop(app_handle, command_rx, bindings, init_tx));
        let initial_result = init_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("hotkey worker initialization failed"))?;
        MANAGER
            .set(HotkeyManager { command_tx })
            .map_err(|_| anyhow::anyhow!("hotkey manager already initialized"))?;
        if let Err(error) = initial_result {
            tracing::warn!(
                "initial hotkey registration failed; settings can be changed in the app: {error}"
            );
        }
        Ok(())
    }

    pub fn reconfigure(bindings: HotkeySet) -> Result<(), String> {
        let manager = MANAGER
            .get()
            .ok_or_else(|| "ホットキー管理が初期化されていません。".to_string())?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        manager
            .command_tx
            .send(WorkerCommand::Reconfigure {
                bindings,
                reply: reply_tx,
            })
            .map_err(|_| "ホットキー管理スレッドへ接続できません。".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "ホットキー再登録結果を取得できません。".to_string())?
    }

    pub fn set_cancel_enabled(enabled: bool) -> Result<(), String> {
        let manager = MANAGER
            .get()
            .ok_or_else(|| "ホットキー管理が初期化されていません。".to_string())?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        manager
            .command_tx
            .send(WorkerCommand::SetCancelEnabled {
                enabled,
                reply: reply_tx,
            })
            .map_err(|_| "ホットキー管理スレッドへ接続できません。".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "Escape監視の切替結果を取得できません。".to_string())?
    }

    fn worker_loop(
        app: AppHandle,
        command_rx: Receiver<WorkerCommand>,
        initial: HotkeySet,
        init_tx: SyncSender<Result<(), String>>,
    ) {
        let mut current = initial;
        let mut cancel_registered = false;
        let initial_result = register_set(&current);
        let mut current_registered = initial_result.is_ok();
        let _ = init_tx.send(initial_result.clone());
        if let Err(error) = initial_result {
            tracing::error!("initial hotkey registration failed: {error}");
        }

        if current_registered {
            tracing::info!(
                "Global hotkeys registered: {}, {}, {}, {}, {}",
                current.push_to_talk.display(),
                current.hands_free_raw.display(),
                current.hands_free_polish.display(),
                current.learn_selected.display(),
                current.voice_edit_selected.display()
            );
        }

        let mut msg = MSG::default();
        loop {
            loop {
                match command_rx.try_recv() {
                    Ok(WorkerCommand::Reconfigure { bindings, reply }) => {
                        let result = if current_registered {
                            replace_set(&current, &bindings)
                        } else {
                            register_set(&bindings)
                        };
                        if result.is_ok() {
                            current = bindings;
                            current_registered = true;
                        }
                        let _ = reply.send(result);
                    }
                    Ok(WorkerCommand::SetCancelEnabled { enabled, reply }) => {
                        let result = set_cancel_registration(enabled, &mut cancel_registered);
                        let _ = reply.send(result);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        unregister_all();
                        return;
                    }
                }
            }

            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                if msg.message != WM_HOTKEY {
                    continue;
                }
                match msg.wParam.0 as i32 {
                    HOTKEY_PUSH_TO_TALK => {
                        let _ = app.emit("hotkey://push-to-talk-down", ());
                        if !PTT_POLLING.swap(true, Ordering::AcqRel) {
                            let app_release = app.clone();
                            let vk = virtual_key(&current.push_to_talk).unwrap_or_default() as i32;
                            std::thread::spawn(move || {
                                poll_key_release(&app_release, vk);
                                PTT_POLLING.store(false, Ordering::Release);
                            });
                        }
                    }
                    HOTKEY_HANDS_FREE_RAW => {
                        let _ = app.emit("hotkey://hands-free-raw-toggle", ());
                    }
                    HOTKEY_HANDS_FREE_POLISH => {
                        let _ = app.emit("hotkey://hands-free-polish-toggle", ());
                    }
                    HOTKEY_LEARN_SELECTED => {
                        let _ = app.emit("hotkey://learn-selected", ());
                    }
                    HOTKEY_VOICE_EDIT_SELECTED => {
                        let _ = app.emit("hotkey://voice-edit-selected", ());
                    }
                    HOTKEY_CANCEL_RECORDING if cancel_registered => {
                        // 一回の押下で先に解除する。長押し・連打による重複イベントを防ぐ。
                        let _ = unsafe { UnregisterHotKey(None, HOTKEY_CANCEL_RECORDING) };
                        cancel_registered = false;
                        // イベント配送より先にバックエンドから見える意図を立てる。
                        // PTT-up等がsession_actionを先に取得しても通常停止を優先させない。
                        app.state::<crate::state::AppState>().request_cancellation();
                        let _ = app.emit("hotkey://cancel-recording", ());
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn set_cancel_registration(enabled: bool, registered: &mut bool) -> Result<(), String> {
        if enabled == *registered {
            return Ok(());
        }
        if enabled {
            unsafe {
                RegisterHotKey(
                    None,
                    HOTKEY_CANCEL_RECORDING,
                    MOD_NOREPEAT,
                    windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 as u32,
                )
            }
            .map_err(|error| {
                format!(
                    "録音キャンセル用のEscapeを登録できません。ほかのアプリで使用されている可能性があります: {error}"
                )
            })?;
            *registered = true;
        } else {
            unsafe { UnregisterHotKey(None, HOTKEY_CANCEL_RECORDING) }
                .map_err(|error| format!("録音キャンセル用のEscapeを解除できません: {error}"))?;
            *registered = false;
        }
        Ok(())
    }

    fn replace_set(current: &HotkeySet, next: &HotkeySet) -> Result<(), String> {
        super::replace_registered_set(current, next, &mut unregister_regular, &mut register_set)
    }

    fn register_set(bindings: &HotkeySet) -> Result<(), String> {
        let entries = [
            (HOTKEY_PUSH_TO_TALK, &bindings.push_to_talk),
            (HOTKEY_HANDS_FREE_RAW, &bindings.hands_free_raw),
            (HOTKEY_HANDS_FREE_POLISH, &bindings.hands_free_polish),
            (HOTKEY_LEARN_SELECTED, &bindings.learn_selected),
            (HOTKEY_VOICE_EDIT_SELECTED, &bindings.voice_edit_selected),
        ];
        let mut registered = Vec::new();
        for (id, binding) in entries {
            let modifiers = modifiers(binding);
            let vk = virtual_key(binding)
                .ok_or_else(|| format!("未対応のショートカットキーです: {}", binding.key))?;
            if let Err(error) = unsafe { RegisterHotKey(None, id, modifiers, vk) } {
                for registered_id in registered {
                    let _ = unsafe { UnregisterHotKey(None, registered_id) };
                }
                return Err(format!(
                    "{} を登録できません。ほかのアプリで使用されている可能性があります: {error}",
                    binding.display()
                ));
            }
            registered.push(id);
        }
        Ok(())
    }

    fn unregister_all() {
        unregister_regular();
        let _ = unsafe { UnregisterHotKey(None, HOTKEY_CANCEL_RECORDING) };
    }

    fn unregister_regular() {
        for id in [
            HOTKEY_PUSH_TO_TALK,
            HOTKEY_HANDS_FREE_RAW,
            HOTKEY_HANDS_FREE_POLISH,
            HOTKEY_LEARN_SELECTED,
            HOTKEY_VOICE_EDIT_SELECTED,
        ] {
            let _ = unsafe { UnregisterHotKey(None, id) };
        }
    }

    fn modifiers(binding: &HotkeyBinding) -> HOT_KEY_MODIFIERS {
        let mut value = MOD_NOREPEAT;
        if binding.ctrl {
            value |= MOD_CONTROL;
        }
        if binding.alt {
            value |= MOD_ALT;
        }
        if binding.shift {
            value |= MOD_SHIFT;
        }
        value
    }

    fn virtual_key(binding: &HotkeyBinding) -> Option<u32> {
        let key = binding.key.as_str();
        if key.len() == 1 {
            let c = key.chars().next()?;
            if c.is_ascii_alphanumeric() {
                return Some(c.to_ascii_uppercase() as u32);
            }
        }
        if let Some(number) = key.strip_prefix('F').and_then(|s| s.parse::<u32>().ok()) {
            if (1..=24).contains(&number) {
                return Some(0x70 + number - 1);
            }
        }
        match key {
            "Space" => Some(0x20),
            "Enter" => Some(0x0D),
            "Tab" => Some(0x09),
            "Esc" => Some(0x1B),
            "ArrowUp" => Some(0x26),
            "ArrowDown" => Some(0x28),
            "ArrowLeft" => Some(0x25),
            "ArrowRight" => Some(0x27),
            "Backspace" => Some(0x08),
            "Delete" => Some(0x2E),
            "Insert" => Some(0x2D),
            "Home" => Some(0x24),
            "End" => Some(0x23),
            "PageUp" => Some(0x21),
            "PageDown" => Some(0x22),
            _ => None,
        }
    }

    fn poll_key_release(app: &AppHandle, vk: i32) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            let is_down = unsafe { GetAsyncKeyState(vk) } < 0;
            if !is_down {
                std::thread::sleep(Duration::from_millis(30));
                let _ = app.emit("hotkey://push-to-talk-up", ());
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        tracing::warn!("push-to-talk release polling timed out (30s)");
        let _ = app.emit("hotkey://push-to-talk-up", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(key: &str) -> HotkeyBinding {
        HotkeyBinding {
            key: key.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn normalizes_supported_key_names() {
        assert_eq!(normalize_binding(&binding("escape")).unwrap().key, "Esc");
        assert_eq!(normalize_binding(&binding("f24")).unwrap().key, "F24");
        assert_eq!(normalize_binding(&binding("a")).unwrap().key, "A");
        assert_eq!(normalize_binding(&binding("pgdn")).unwrap().key, "PageDown");
    }

    #[test]
    fn rejects_unsupported_keys() {
        assert!(normalize_binding(&binding(";")).is_err());
        assert!(normalize_binding(&binding("F25")).is_err());
        assert!(normalize_binding(&binding("Control")).is_err());
    }

    #[test]
    fn rejects_duplicate_bindings() {
        let duplicate = HotkeyBinding::push_to_talk_default();
        let set = HotkeySet::new(
            duplicate.clone(),
            HotkeyBinding::hands_free_raw_default(),
            HotkeyBinding::hands_free_polish_default(),
            HotkeyBinding::learn_selected_default(),
            duplicate,
        );
        assert!(set.validate_and_normalize().is_err());
    }

    #[test]
    fn rejects_duplicates_across_every_regular_binding_pair() {
        for (left, right) in [
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4),
            (1, 2),
            (1, 3),
            (1, 4),
            (2, 3),
            (2, 4),
            (3, 4),
        ] {
            let mut bindings = [
                binding("F4"),
                binding("F6"),
                binding("F7"),
                binding("F8"),
                binding("F9"),
            ];
            bindings[right] = bindings[left].clone();
            let [push_to_talk, hands_free_raw, hands_free_polish, learn_selected, voice_edit_selected] =
                bindings;
            assert!(HotkeySet::new(
                push_to_talk,
                hands_free_raw,
                hands_free_polish,
                learn_selected,
                voice_edit_selected,
            )
            .validate_and_normalize()
            .is_err());
        }
    }

    #[test]
    fn accepts_single_key_binding() {
        let set = HotkeySet::new(
            binding("F4"),
            binding("F6"),
            binding("F7"),
            binding("F8"),
            binding("M"),
        );
        assert!(set.validate_and_normalize().is_ok());
    }

    #[test]
    fn failed_reconfiguration_unregisters_partial_next_and_restores_previous_set() {
        let current = HotkeySet::new(
            binding("F4"),
            binding("F6"),
            binding("F7"),
            binding("F8"),
            binding("F9"),
        );
        let next = HotkeySet::new(
            binding("F9"),
            binding("F10"),
            binding("F11"),
            binding("F12"),
            binding("F13"),
        );
        let mut unregister_calls = 0;
        let mut registered_first_keys = Vec::new();
        let result =
            replace_registered_set(&current, &next, &mut || unregister_calls += 1, &mut |set| {
                registered_first_keys.push(set.push_to_talk.key.clone());
                if set == &next {
                    Err("next failed".to_string())
                } else {
                    Ok(())
                }
            });
        assert_eq!(result.unwrap_err(), "next failed");
        assert_eq!(unregister_calls, 2);
        assert_eq!(registered_first_keys, vec!["F9", "F4"]);
    }
}
