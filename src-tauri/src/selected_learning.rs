use std::{
    cmp::Ordering,
    collections::HashSet,
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::{
    context::FocusedWindowTarget,
    corrections::CorrectionStore,
    local_data::{HistoryEntry, HistoryStatus, OperationKind},
    selection::SelectionWarning,
    settings::CorrectionLearningMode,
    state::{Mode, RecordingState},
};

pub const CANDIDATE_WINDOW_SECS: u64 = 30 * 60;
pub const PENDING_EXPIRY_SECS: u64 = 10 * 60;
pub const MAX_CANDIDATES: usize = 5;

static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SelectedCorrectionCandidate {
    pub id: String,
    pub final_text: String,
    pub mode: Mode,
    pub polish_preset: String,
    pub app_process: String,
    pub created_at: u64,
    pub same_app: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PrepareSelectedCorrectionResult {
    pub selected_text: String,
    pub candidates: Vec<SelectedCorrectionCandidate>,
    pub token: String,
    pub warning: Option<SelectionWarning>,
}

pub struct PendingSelectedLearning {
    token: String,
    selected_text: Option<String>,
    candidates: Vec<SelectedCorrectionCandidate>,
    created_at: u64,
    target: FocusedWindowTarget,
    expired: bool,
}

#[derive(Clone)]
pub struct ResolvedPendingSelection {
    pub selected_text: String,
    pub candidate: SelectedCorrectionCandidate,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn new_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = TOKEN_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    format!("selected-{nanos:x}-{counter:x}")
}

fn process_basename(value: &str) -> &str {
    value
        .rsplit(['\\', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or("")
}

fn same_app(left: &str, right: &str) -> bool {
    let left = process_basename(left);
    let right = process_basename(right);
    !left.is_empty() && !right.is_empty() && left.eq_ignore_ascii_case(right)
}

pub fn ensure_learning_enabled(mode: CorrectionLearningMode) -> Result<(), String> {
    if mode == CorrectionLearningMode::Ask {
        Ok(())
    } else {
        Err("修正学習はオフです。設定で「保存前に確認」を選択してください。".to_string())
    }
}

pub fn ensure_session_idle(
    recording_state: &RecordingState,
    has_session: bool,
) -> Result<(), String> {
    if matches!(recording_state, RecordingState::Idle) && !has_session {
        Ok(())
    } else {
        Err("録音中または処理中は、選択テキストから修正を学習できません。".to_string())
    }
}

pub fn select_candidates(
    history: &[HistoryEntry],
    store: &CorrectionStore,
    focused_process: &str,
    now: u64,
) -> Vec<SelectedCorrectionCandidate> {
    let learned = store
        .items
        .iter()
        .map(|item| item.source_history_id.as_str())
        .collect::<HashSet<_>>();
    let mut candidates = history
        .iter()
        .filter(|item| {
            item.status == HistoryStatus::Success
                && item.operation_kind == OperationKind::Dictation
                && !item.final_text.trim().is_empty()
                && item.created_at <= now
                && now - item.created_at <= CANDIDATE_WINDOW_SECS
                && !learned.contains(item.id.as_str())
        })
        .map(|item| SelectedCorrectionCandidate {
            id: item.id.clone(),
            final_text: item.final_text.clone(),
            mode: item.mode.clone(),
            polish_preset: item.polish_preset.clone(),
            app_process: item.app_process.clone(),
            created_at: item.created_at,
            same_app: same_app(&item.app_process, focused_process),
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| match right.same_app.cmp(&left.same_app) {
        Ordering::Equal => match right.created_at.cmp(&left.created_at) {
            Ordering::Equal => left.id.cmp(&right.id),
            ordering => ordering,
        },
        ordering => ordering,
    });
    candidates.truncate(MAX_CANDIDATES);
    candidates
}

pub fn replace_pending(
    pending: &mut Option<PendingSelectedLearning>,
    selected_text: String,
    candidates: Vec<SelectedCorrectionCandidate>,
    now: u64,
    target: FocusedWindowTarget,
    warning: Option<SelectionWarning>,
) -> PrepareSelectedCorrectionResult {
    let token = new_token();
    *pending = Some(PendingSelectedLearning {
        token: token.clone(),
        selected_text: Some(selected_text.clone()),
        candidates: candidates.clone(),
        created_at: now,
        target,
        expired: false,
    });
    PrepareSelectedCorrectionResult {
        selected_text,
        candidates,
        token,
        warning,
    }
}

pub fn all_candidates_match_selected(
    candidates: &[SelectedCorrectionCandidate],
    selected_text: &str,
) -> bool {
    !candidates.is_empty()
        && candidates
            .iter()
            .all(|candidate| candidate.final_text == selected_text)
}

pub fn resolve_pending(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
    history_id: &str,
    now: u64,
) -> Result<ResolvedPendingSelection, String> {
    let Some(current) = pending.as_mut() else {
        return Err("選択テキストの学習確認は期限切れかキャンセル済みです。".to_string());
    };
    if current.token != token {
        return Err("選択テキストの学習確認トークンが一致しません。".to_string());
    }
    if current.expired || now.saturating_sub(current.created_at) > PENDING_EXPIRY_SECS {
        // 本文は保持し続けず、明示cancel時のfocus復帰に必要なtargetとtokenだけを残す。
        current.selected_text = None;
        current.candidates.clear();
        current.expired = true;
        return Err("選択テキストの学習確認は10分で期限切れになりました。".to_string());
    }
    let candidate = current
        .candidates
        .iter()
        .find(|candidate| candidate.id == history_id)
        .cloned()
        .ok_or_else(|| "選択した履歴候補はこの確認操作に含まれていません。".to_string())?;
    Ok(ResolvedPendingSelection {
        selected_text: current
            .selected_text
            .clone()
            .ok_or_else(|| "選択テキストの学習確認は10分で期限切れになりました。".to_string())?,
        candidate,
    })
}

pub fn cancel_pending(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
) -> Result<FocusedWindowTarget, String> {
    match pending.as_ref() {
        Some(current) if current.token == token => {
            let target = current.target.clone();
            *pending = None;
            Ok(target)
        }
        _ => Err("選択テキストの学習確認は期限切れかキャンセル済みです。".to_string()),
    }
}

pub fn consume_pending(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
) -> Option<FocusedWindowTarget> {
    if pending
        .as_ref()
        .is_some_and(|current| current.token == token)
    {
        pending.take().map(|current| current.target)
    } else {
        None
    }
}

pub fn candidate_matches_history(
    candidate: &SelectedCorrectionCandidate,
    entry: &HistoryEntry,
) -> bool {
    candidate.id == entry.id
        && candidate.final_text == entry.final_text
        && candidate.mode == entry.mode
        && candidate.polish_preset == entry.polish_preset
        && candidate.app_process == entry.app_process
        && candidate.created_at == entry.created_at
        && entry.status == HistoryStatus::Success
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        corrections::{CorrectionRecord, CorrectionStatus},
        polish::PolishState,
    };

    fn history(id: &str, created_at: u64, app: &str, status: HistoryStatus) -> HistoryEntry {
        HistoryEntry {
            id: id.into(),
            raw_text: format!("raw-{id}"),
            final_text: format!("final-{id}"),
            mode: Mode::Raw,
            duration_ms: 1,
            created_at,
            error: None,
            status,
            polish_state: PolishState::NotRequested,
            pinned: false,
            polish_preset: "memo".into(),
            app_process: app.into(),
            operation_kind: OperationKind::Dictation,
        }
    }

    fn learned(history_id: &str) -> CorrectionRecord {
        CorrectionRecord {
            id: format!("corr-{history_id}"),
            source_history_id: history_id.into(),
            raw_text: "raw".into(),
            original_text: "before".into(),
            corrected_text: "after".into(),
            mode: Mode::Raw,
            polish_preset: "memo".into(),
            app_process: "app.exe".into(),
            classification: crate::corrections::CorrectionClassification::Minor,
            status: CorrectionStatus::Active,
            artifacts: vec![crate::corrections::CorrectionArtifact::None],
            created_at: 1,
            updated_at: 1,
        }
    }

    fn target() -> FocusedWindowTarget {
        FocusedWindowTarget {
            hwnd: 1,
            process_id: 2,
            process_name: "app.exe".into(),
            window_title: String::new(),
        }
    }

    #[test]
    fn candidates_filter_age_status_future_and_learned_history() {
        let now = 10_000;
        let items = vec![
            history(
                "fresh",
                now - CANDIDATE_WINDOW_SECS,
                "app.exe",
                HistoryStatus::Success,
            ),
            history(
                "old",
                now - CANDIDATE_WINDOW_SECS - 1,
                "app.exe",
                HistoryStatus::Success,
            ),
            history("future", now + 1, "app.exe", HistoryStatus::Success),
            history("error", now, "app.exe", HistoryStatus::Error),
            history("learned", now, "app.exe", HistoryStatus::Success),
        ];
        let store = CorrectionStore {
            items: vec![learned("learned")],
            ..Default::default()
        };
        let result = select_candidates(&items, &store, "app.exe", now);
        assert_eq!(
            result
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["fresh"]
        );

        let mut undone = learned("fresh");
        undone.status = CorrectionStatus::Undone;
        assert!(select_candidates(
            &[history("fresh", now, "app.exe", HistoryStatus::Success)],
            &CorrectionStore {
                items: vec![undone],
                ..Default::default()
            },
            "app.exe",
            now,
        )
        .is_empty());
    }

    #[test]
    fn candidates_rank_same_basename_then_newest_then_id_and_cap_five() {
        let now = 10_000;
        let items = vec![
            history("z", now - 1, "other.exe", HistoryStatus::Success),
            history("b", now - 2, r"C:\Apps\NOTEPAD.EXE", HistoryStatus::Success),
            history("a", now - 2, "notepad.exe", HistoryStatus::Success),
            history("c", now - 3, "notepad.exe", HistoryStatus::Success),
            history("d", now - 4, "notepad.exe", HistoryStatus::Success),
            history("e", now - 5, "notepad.exe", HistoryStatus::Success),
            history("f", now - 6, "notepad.exe", HistoryStatus::Success),
        ];
        let result = select_candidates(&items, &CorrectionStore::default(), "notepad.exe", now);
        assert_eq!(
            result
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "d", "e"]
        );
        assert!(result.iter().all(|item| item.same_app));
    }

    #[test]
    fn ask_off_expiry_tamper_cancel_and_replacement_are_enforced() {
        assert!(ensure_learning_enabled(CorrectionLearningMode::Off).is_err());
        assert!(ensure_learning_enabled(CorrectionLearningMode::Ask).is_ok());
        let candidate = SelectedCorrectionCandidate {
            id: "h1".into(),
            final_text: "original".into(),
            mode: Mode::Raw,
            polish_preset: "memo".into(),
            app_process: "app.exe".into(),
            created_at: 1,
            same_app: true,
        };
        let mut pending = None;
        let first = replace_pending(
            &mut pending,
            "selected".into(),
            vec![candidate.clone()],
            100,
            target(),
            None,
        );
        assert!(resolve_pending(&mut pending, "tampered", "h1", 100).is_err());
        assert!(resolve_pending(
            &mut pending,
            "tampered",
            "h1",
            100 + PENDING_EXPIRY_SECS + 1
        )
        .is_err());
        assert!(!pending.as_ref().unwrap().expired);
        assert!(pending.as_ref().unwrap().selected_text.is_some());
        assert!(resolve_pending(&mut pending, &first.token, "tampered", 100).is_err());
        let second = replace_pending(
            &mut pending,
            "new".into(),
            vec![candidate],
            200,
            target(),
            None,
        );
        assert!(resolve_pending(&mut pending, &first.token, "h1", 200).is_err());
        assert!(resolve_pending(
            &mut pending,
            &second.token,
            "h1",
            200 + PENDING_EXPIRY_SECS + 1
        )
        .is_err());
        assert!(pending.is_some());
        let expired = pending.as_ref().unwrap();
        assert!(expired.expired);
        assert!(expired.selected_text.is_none());
        assert!(expired.candidates.is_empty());
        assert_eq!(
            cancel_pending(&mut pending, &second.token).unwrap(),
            target()
        );
        assert!(pending.is_none());
        let third = replace_pending(&mut pending, "new".into(), Vec::new(), 300, target(), None);
        cancel_pending(&mut pending, &third.token).unwrap();
        assert!(pending.is_none());
    }

    #[test]
    fn recording_processing_and_active_session_are_rejected() {
        assert!(ensure_session_idle(&RecordingState::Idle, false).is_ok());
        assert!(ensure_session_idle(&RecordingState::Idle, true).is_err());
        assert!(ensure_session_idle(&RecordingState::Recording, false).is_err());
        assert!(ensure_session_idle(&RecordingState::Processing, false).is_err());
    }

    #[test]
    fn unchanged_is_rejected_only_when_every_candidate_matches() {
        let candidate = |id: &str, text: &str| SelectedCorrectionCandidate {
            id: id.into(),
            final_text: text.into(),
            mode: Mode::Raw,
            polish_preset: "memo".into(),
            app_process: "app.exe".into(),
            created_at: 1,
            same_app: true,
        };
        assert!(all_candidates_match_selected(
            &[candidate("a", "same")],
            "same"
        ));
        assert!(!all_candidates_match_selected(
            &[candidate("a", "same"), candidate("b", "different")],
            "same"
        ));
        assert!(!all_candidates_match_selected(&[], "same"));
    }
}
