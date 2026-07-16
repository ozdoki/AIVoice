use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::{
    context::FocusedWindowTarget,
    selection::{SelectionCapture, SelectionMethod, SelectionWarning},
    state::RecordingState,
};

pub const PREVIEW_TTL_SECS: u64 = 10 * 60;
pub const ASR_TIMEOUT_SECS: u64 = 90;
pub const EDIT_TIMEOUT_SECS: u64 = 90;
static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct ActiveSelectedVoiceEdit {
    pub target: FocusedWindowTarget,
    pub original_text: String,
    pub selection_method: SelectionMethod,
    pub selection_warning: Option<SelectionWarning>,
    pub recovery_id: String,
}

pub struct PendingSelectedVoiceEdit {
    token: String,
    original_text: Option<String>,
    instruction: Option<String>,
    proposal: Option<String>,
    target: FocusedWindowTarget,
    selection_method: SelectionMethod,
    created_at: u64,
    expired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectedVoiceEditPreview {
    pub token: String,
    pub original_text: String,
    pub instruction: String,
    pub proposal: String,
    pub app_process: String,
    pub replace_available: bool,
    pub selection_warning: Option<SelectionWarning>,
    pub history_id: Option<String>,
}

#[derive(Clone)]
pub struct ResolvedSelectedVoiceEdit {
    pub original_text: String,
    pub instruction: String,
    pub proposal: String,
    pub target: FocusedWindowTarget,
    pub selection_method: SelectionMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceDecision {
    Replace,
    ClipboardCaptureUnsupported,
    TargetMismatch,
    SelectionMismatch,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("voice-edit-{nanos:x}-{counter:x}")
}

pub fn ensure_operation_available(
    recording_state: &RecordingState,
    has_session: bool,
    has_selected_learning: bool,
    has_active_voice_edit: bool,
    has_pending_voice_edit: bool,
) -> Result<(), String> {
    if matches!(recording_state, RecordingState::Idle)
        && !has_session
        && !has_selected_learning
        && !has_active_voice_edit
        && !has_pending_voice_edit
    {
        Ok(())
    } else {
        Err("録音、処理、または別の選択テキスト操作が進行中です。".to_string())
    }
}

pub fn replace_pending(
    pending: &mut Option<PendingSelectedVoiceEdit>,
    active: ActiveSelectedVoiceEdit,
    instruction: String,
    proposal: String,
    history_id: Option<String>,
    now: u64,
) -> SelectedVoiceEditPreview {
    let token = token();
    let replace_available = active.selection_method == SelectionMethod::UiaTextPattern;
    let preview = SelectedVoiceEditPreview {
        token: token.clone(),
        original_text: active.original_text.clone(),
        instruction: instruction.clone(),
        proposal: proposal.clone(),
        app_process: active.target.process_name.clone(),
        replace_available,
        selection_warning: active.selection_warning,
        history_id,
    };
    *pending = Some(PendingSelectedVoiceEdit {
        token,
        original_text: Some(active.original_text),
        instruction: Some(instruction),
        proposal: Some(proposal),
        target: active.target,
        selection_method: active.selection_method,
        created_at: now,
        expired: false,
    });
    preview
}

pub fn resolve_pending(
    pending: &mut Option<PendingSelectedVoiceEdit>,
    token: &str,
    now: u64,
) -> Result<ResolvedSelectedVoiceEdit, String> {
    let Some(current) = pending.as_mut() else {
        return Err("選択音声編集のpreviewは終了済みです。".to_string());
    };
    if current.token != token {
        return Err("選択音声編集の確認トークンが一致しません。".to_string());
    }
    if current.expired || now.saturating_sub(current.created_at) > PREVIEW_TTL_SECS {
        current.original_text = None;
        current.instruction = None;
        current.proposal = None;
        current.expired = true;
        return Err("選択音声編集のpreviewは10分で期限切れになりました。".to_string());
    }
    Ok(ResolvedSelectedVoiceEdit {
        original_text: current
            .original_text
            .clone()
            .ok_or_else(|| "選択音声編集のpreviewは10分で期限切れになりました。".to_string())?,
        instruction: current
            .instruction
            .clone()
            .ok_or_else(|| "選択音声編集のpreviewは10分で期限切れになりました。".to_string())?,
        proposal: current
            .proposal
            .clone()
            .ok_or_else(|| "選択音声編集のpreviewは10分で期限切れになりました。".to_string())?,
        target: current.target.clone(),
        selection_method: current.selection_method,
    })
}

pub fn cancel_pending(
    pending: &mut Option<PendingSelectedVoiceEdit>,
    token: &str,
) -> Result<FocusedWindowTarget, String> {
    match pending.as_ref() {
        Some(current) if current.token == token => {
            let target = current.target.clone();
            *pending = None;
            Ok(target)
        }
        _ => Err("選択音声編集のpreviewは終了済みです。".to_string()),
    }
}

pub fn finish_pending(
    pending: &mut Option<PendingSelectedVoiceEdit>,
    token: &str,
) -> Result<(), String> {
    match pending.as_ref() {
        Some(current) if current.token == token => {
            *pending = None;
            Ok(())
        }
        _ => Err("選択音声編集のpreviewは終了済みです。".to_string()),
    }
}

pub fn expire_pending(pending: &mut Option<PendingSelectedVoiceEdit>, token: &str) -> bool {
    let Some(current) = pending.as_mut() else {
        return false;
    };
    if current.token != token {
        return false;
    }
    current.original_text = None;
    current.instruction = None;
    current.proposal = None;
    current.expired = true;
    true
}

pub fn replacement_decision(
    pending: &ResolvedSelectedVoiceEdit,
    current: &SelectionCapture,
) -> ReplaceDecision {
    if pending.selection_method != SelectionMethod::UiaTextPattern
        || current.method != SelectionMethod::UiaTextPattern
    {
        return ReplaceDecision::ClipboardCaptureUnsupported;
    }
    if pending.target.hwnd != current.target.hwnd
        || pending.target.process_id != current.target.process_id
    {
        return ReplaceDecision::TargetMismatch;
    }
    if pending.original_text != current.text {
        return ReplaceDecision::SelectionMismatch;
    }
    ReplaceDecision::Replace
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(hwnd: isize) -> FocusedWindowTarget {
        FocusedWindowTarget {
            hwnd,
            process_id: 7,
            process_name: "notepad.exe".into(),
            window_title: String::new(),
        }
    }

    fn active(method: SelectionMethod) -> ActiveSelectedVoiceEdit {
        ActiveSelectedVoiceEdit {
            target: target(1),
            original_text: "original 123 Tanaka".into(),
            selection_method: method,
            selection_warning: None,
            recovery_id: "rec".into(),
        }
    }

    #[test]
    fn operation_is_isolated_from_sessions_and_selected_learning() {
        assert!(ASR_TIMEOUT_SECS > 0 && ASR_TIMEOUT_SECS <= 120);
        assert!(EDIT_TIMEOUT_SECS > 0 && EDIT_TIMEOUT_SECS <= 120);
        assert!(
            ensure_operation_available(&RecordingState::Idle, false, false, false, false).is_ok()
        );
        for args in [
            (RecordingState::Recording, false, false, false, false),
            (RecordingState::Processing, false, false, false, false),
            (RecordingState::Idle, true, false, false, false),
            (RecordingState::Idle, false, true, false, false),
            (RecordingState::Idle, false, false, true, false),
            (RecordingState::Idle, false, false, false, true),
        ] {
            assert!(ensure_operation_available(&args.0, args.1, args.2, args.3, args.4).is_err());
        }
    }

    #[test]
    fn token_expiry_scrubs_text_but_keeps_focus_target_for_cancel() {
        let mut pending = None;
        let preview = replace_pending(
            &mut pending,
            active(SelectionMethod::UiaTextPattern),
            "shorten".into(),
            "proposal".into(),
            None,
            10,
        );
        assert!(resolve_pending(&mut pending, "tampered", 10).is_err());
        assert!(!expire_pending(&mut pending, "tampered"));
        assert!(expire_pending(&mut pending, &preview.token));
        assert!(resolve_pending(&mut pending, &preview.token, 10).is_err());
        let state = pending.as_ref().unwrap();
        assert!(
            state.original_text.is_none()
                && state.instruction.is_none()
                && state.proposal.is_none()
        );
        assert_eq!(
            cancel_pending(&mut pending, &preview.token).unwrap(),
            target(1)
        );
    }

    #[test]
    fn replace_requires_uia_exact_target_and_text() {
        let mut pending = None;
        let preview = replace_pending(
            &mut pending,
            active(SelectionMethod::UiaTextPattern),
            "shorten".into(),
            "proposal".into(),
            None,
            10,
        );
        let resolved = resolve_pending(&mut pending, &preview.token, 10).unwrap();
        let capture = |method, hwnd, text: &str| SelectionCapture {
            target: target(hwnd),
            text: text.into(),
            method,
            warning: None,
        };
        assert_eq!(
            replacement_decision(
                &resolved,
                &capture(SelectionMethod::UiaTextPattern, 1, "original 123 Tanaka")
            ),
            ReplaceDecision::Replace
        );
        assert_eq!(
            replacement_decision(
                &resolved,
                &capture(SelectionMethod::ClipboardFallback, 1, "original 123 Tanaka")
            ),
            ReplaceDecision::ClipboardCaptureUnsupported
        );
        assert_eq!(
            replacement_decision(
                &resolved,
                &capture(SelectionMethod::UiaTextPattern, 2, "original 123 Tanaka")
            ),
            ReplaceDecision::TargetMismatch
        );
        assert_eq!(
            replacement_decision(
                &resolved,
                &capture(SelectionMethod::UiaTextPattern, 1, "changed")
            ),
            ReplaceDecision::SelectionMismatch
        );
    }
}
